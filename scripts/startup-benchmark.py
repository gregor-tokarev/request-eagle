#!/usr/bin/env python3
"""Measure how long desktop apps take to start, by watching the screen.

Each app is launched on its own empty virtual display while the display is
captured at 60 frames per second. Two moments are timed from the launch:

  first paint  the first frame that shows a window
  ready        the frame of the last visible change before the screen goes quiet

The screen counts as quiet after QUIET seconds without a change larger than a
blinking caret. Apps are measured in turn, so background load affects them
alike. Requires Linux, Xvfb, ffmpeg and Pillow.

    scripts/startup-benchmark.py --runs 10 --out results.json \\
        "eagle=target/release/request-eagle" \\
        "postman=/opt/Postman/Postman --no-sandbox"
"""

import argparse
import json
import os
import signal
import statistics
import subprocess
import sys
import time

from PIL import Image, ImageChops

WIDTH, HEIGHT = 1280, 800
FRAME_BYTES = WIDTH * HEIGHT

# A change smaller than this is a caret or a hover, not the app drawing itself.
CHANGED_PIXELS = 300
# A window covers at least this share of the screen.
WINDOW_SHARE = 0.01
QUIET = 2.0
TIMEOUT = 90.0


def changed_pixels(before, after):
    difference = ImageChops.difference(before, after)

    return FRAME_BYTES - difference.histogram()[0]


def read_frame(capture):
    data = capture.stdout.read(FRAME_BYTES)

    if len(data) < FRAME_BYTES:
        raise RuntimeError("The screen capture ended early.")

    return Image.frombytes("L", (WIDTH, HEIGHT), data), time.monotonic()


def start_display():
    """Start Xvfb on a display it picks itself, and wait until it accepts clients."""
    reader, writer = os.pipe()

    server = subprocess.Popen(
        ["Xvfb", "-displayfd", str(writer), "-screen", "0", f"{WIDTH}x{HEIGHT}x24", "-nolisten", "tcp"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        pass_fds=[writer],
        start_new_session=True,
    )
    os.close(writer)

    # Xvfb writes the display number once it is ready, or closes the pipe if it fails.
    with os.fdopen(reader) as pipe:
        number = pipe.readline().strip()

    if not number:
        server.wait()
        raise RuntimeError(f"Xvfb did not start (exit code {server.returncode}).")

    return server, f":{number}"


def stop(process):
    if process.poll() is not None:
        return

    os.killpg(process.pid, signal.SIGTERM)

    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()


def measure(command, frame_path=None):
    """Launch one app on a fresh display and time its first paint and readiness."""
    server, display = start_display()

    capture = subprocess.Popen(
        [
            "ffmpeg", "-loglevel", "error", "-fflags", "nobuffer",
            "-f", "x11grab", "-framerate", "60", "-draw_mouse", "0",
            "-video_size", f"{WIDTH}x{HEIGHT}", "-i", display,
            "-pix_fmt", "gray", "-f", "rawvideo", "-flush_packets", "1", "-",
        ],
        stdout=subprocess.PIPE,
        start_new_session=True,
    )

    app = None

    try:
        # The empty display is the baseline a window is compared against.
        for _ in range(10):
            empty, _ = read_frame(capture)

        environment = dict(os.environ, DISPLAY=display)
        environment.pop("WAYLAND_DISPLAY", None)

        launched = time.monotonic()
        app = subprocess.Popen(
            f"exec {command}",
            shell=True,
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )

        previous = empty
        first_paint = None
        last_change = None
        last_frame = None

        while True:
            frame, seen = read_frame(capture)

            if seen - launched > TIMEOUT:
                raise RuntimeError(f"No quiet screen within {TIMEOUT:.0f} s.")

            # A closing window would otherwise pass for the last change before a quiet screen.
            if app.poll() is not None:
                raise RuntimeError(f"The app exited during startup (exit code {app.returncode}): {command}")

            if first_paint is None:
                if changed_pixels(empty, frame) < FRAME_BYTES * WINDOW_SHARE:
                    continue

                first_paint = seen

            if changed_pixels(previous, frame) >= CHANGED_PIXELS or last_change is None:
                last_change = seen
                last_frame = frame

            previous = frame

            if seen - last_change >= QUIET:
                break

        if frame_path:
            last_frame.save(frame_path)

        return {
            "first_paint_ms": round((first_paint - launched) * 1000),
            "ready_ms": round((last_change - launched) * 1000),
        }
    finally:
        if app:
            stop(app)

        stop(capture)
        stop(server)
        time.sleep(1.5)


def summarize(runs, key):
    values = [run[key] for run in runs]

    return {"median": round(statistics.median(values)), "min": min(values), "max": max(values)}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("apps", nargs="+", metavar="NAME=COMMAND")
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--out", help="write the results as JSON")
    parser.add_argument("--frames", help="directory for the frame each app was ready on")
    arguments = parser.parse_args()

    apps = dict(app.split("=", 1) for app in arguments.apps)
    runs = {name: [] for name in apps}

    # One unmeasured launch each, so every measured launch starts from a warm file cache.
    for name, command in apps.items():
        measure(command)
        print(f"{name}: warmed up", file=sys.stderr)

    for index in range(arguments.runs):
        for name, command in apps.items():
            frame_path = None

            if arguments.frames and index == 0:
                os.makedirs(arguments.frames, exist_ok=True)
                frame_path = os.path.join(arguments.frames, f"{name}.png")

            run = measure(command, frame_path)
            run["load"] = round(os.getloadavg()[0], 2)
            runs[name].append(run)
            print(f"{name} #{index + 1}: {run}", file=sys.stderr)

    results = {
        name: {
            "command": apps[name],
            "first_paint_ms": summarize(measured, "first_paint_ms"),
            "ready_ms": summarize(measured, "ready_ms"),
            "runs": measured,
        }
        for name, measured in runs.items()
    }

    output = json.dumps(results, indent=2)

    if arguments.out:
        with open(arguments.out, "w") as file:
            file.write(output + "\n")

    print(output)


if __name__ == "__main__":
    main()
