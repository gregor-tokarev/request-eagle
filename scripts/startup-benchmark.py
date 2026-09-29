#!/usr/bin/env python3
"""Measure how long desktop apps take to start, by watching the screen.

Each app is launched on its own empty virtual display while the display is
captured at 60 frames per second. Two moments are timed from the launch:

  first paint  the first frame that shows a window
  ready        the frame of the last visible change before the screen goes quiet

The screen counts as quiet after QUIET seconds without a change larger than a
blinking caret. A stalled app can look quiet too, so the result is rejected
unless every launch of an app ends on the same screen. Each launch waits for
the machine to be idle, and apps are measured in turn. Requires Linux, Xvfb,
ffmpeg and Pillow.

    scripts/startup-benchmark.py --runs 10 --out results.json \\
        "eagle=target/release/request-eagle" \\
        "postman=/opt/Postman/Postman --no-sandbox"
"""

import argparse
import json
import os
import select
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
QUIET = 3.0
TIMEOUT = 90.0
# Launches of one app end on the same screen, give or take this share of it.
SAME_SCREEN = 0.02
# Load per processor above which a launch would be timed against other work.
IDLE_LOAD = 0.15
IDLE_WAIT = 1800.0
# The display and the capture are ready within this long, or they have failed.
DISPLAY_START = 15.0
CAPTURE_START = 15.0


def changed_pixels(before, after):
    difference = ImageChops.difference(before, after)

    return FRAME_BYTES - difference.histogram()[0]


def read_frame(capture, deadline):
    """Read one frame, giving up at the deadline if the capture stalls."""
    chunks = []
    missing = FRAME_BYTES

    while missing:
        waiting = deadline - time.monotonic()

        if waiting <= 0 or not select.select([capture.stdout], [], [], waiting)[0]:
            raise RuntimeError("The screen capture stopped producing frames.")

        chunk = os.read(capture.stdout.fileno(), missing)

        if not chunk:
            raise RuntimeError("The screen capture ended early.")

        chunks.append(chunk)
        missing -= len(chunk)

    return Image.frombytes("L", (WIDTH, HEIGHT), b"".join(chunks)), time.monotonic()


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
        if not select.select([pipe], [], [], DISPLAY_START)[0]:
            stop(server)
            raise RuntimeError(f"Xvfb was not ready within {DISPLAY_START:.0f} s.")

        number = pipe.readline().strip()

    if not number:
        server.wait()
        raise RuntimeError(f"Xvfb did not start (exit code {server.returncode}).")

    return server, f":{number}"


def signal_group(process, number):
    """Signal a process's whole group, and report whether the group still exists."""
    try:
        os.killpg(process.pid, number)
    except ProcessLookupError:
        return False

    return True


def stop(process):
    """Stop a process and its group. A launcher may exit and leave the app it started."""
    if not signal_group(process, signal.SIGTERM):
        return

    deadline = time.monotonic() + 10

    # Polling reaps the leader, which would otherwise keep an empty group alive.
    while process.poll() is None or signal_group(process, 0):
        if time.monotonic() > deadline:
            signal_group(process, signal.SIGKILL)
            break

        time.sleep(0.1)

    process.wait()


def wait_until_idle():
    limit = os.cpu_count() * IDLE_LOAD
    deadline = time.monotonic() + IDLE_WAIT

    while os.getloadavg()[0] > limit:
        if time.monotonic() > deadline:
            raise RuntimeError(f"The machine stayed busy: load {os.getloadavg()[0]:.2f}, limit {limit:.2f}.")

        print(f"waiting for load {os.getloadavg()[0]:.2f} to fall below {limit:.2f}", file=sys.stderr)
        time.sleep(10)


def measure(command):
    """Launch one app on a fresh display and time its first paint and readiness."""
    server, display = start_display()
    capture = None
    app = None

    try:
        capture = subprocess.Popen(
            [
                "ffmpeg", "-loglevel", "error", "-fflags", "nobuffer",
                "-f", "x11grab", "-framerate", "60", "-draw_mouse", "0",
                "-video_size", f"{WIDTH}x{HEIGHT}", "-i", display,
                "-pix_fmt", "gray", "-f", "rawvideo", "-flush_packets", "1", "-",
            ],
            stdout=subprocess.PIPE,
            bufsize=0,
            start_new_session=True,
        )

        # The empty display is the baseline a window is compared against.
        started = time.monotonic()

        for _ in range(10):
            empty, _ = read_frame(capture, started + CAPTURE_START)

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
            frame, seen = read_frame(capture, launched + TIMEOUT)

            if seen - launched >= TIMEOUT:
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

        return {
            "first_paint_ms": round((first_paint - launched) * 1000),
            "ready_ms": round((last_change - launched) * 1000),
            "screen": last_frame,
        }
    finally:
        for process in (app, capture, server):
            if process:
                stop(process)
        time.sleep(1.5)


def ended_elsewhere(runs):
    """List the launches that ended on a different screen than the median launch."""
    ranked = sorted(runs, key=lambda run: run["ready_ms"])
    usual = ranked[len(ranked) // 2]["screen"]

    return [
        index + 1
        for index, run in enumerate(runs)
        if changed_pixels(usual, run["screen"]) > FRAME_BYTES * SAME_SCREEN
    ]


def summarize(runs, key):
    values = [run[key] for run in runs]

    return {"median": round(statistics.median(values)), "min": min(values), "max": max(values)}


def at_least_one(text):
    count = int(text)

    if count < 1:
        raise argparse.ArgumentTypeError("must be at least 1")

    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("apps", nargs="+", metavar="NAME=COMMAND")
    parser.add_argument("--runs", type=at_least_one, default=10)
    parser.add_argument("--out", help="write the results as JSON")
    parser.add_argument("--frames", help="directory for the screen each launch ended on")
    arguments = parser.parse_args()

    apps = {}

    for app in arguments.apps:
        name, separator, command = app.partition("=")

        if not (name and separator and command):
            parser.error(f"expected NAME=COMMAND, got {app!r}")

        if name in apps:
            parser.error(f"the name {name!r} is given twice")

        apps[name] = command

    runs = {name: [] for name in apps}

    # One unmeasured launch each, so every measured launch starts from a warm file cache.
    for name, command in apps.items():
        measure(command)
        print(f"{name}: warmed up", file=sys.stderr)

    for index in range(arguments.runs):
        for name, command in apps.items():
            wait_until_idle()

            run = measure(command)
            run["load"] = round(os.getloadavg()[0], 2)
            runs[name].append(run)
            print(f"{name} #{index + 1}: {run['first_paint_ms']} ms first paint, {run['ready_ms']} ms ready", file=sys.stderr)

    for name, measured in runs.items():
        if arguments.frames:
            os.makedirs(arguments.frames, exist_ok=True)

            for index, run in enumerate(measured):
                run["screen"].save(os.path.join(arguments.frames, f"{name}-{index + 1}.png"))

        strays = ended_elsewhere(measured)

        if strays:
            sys.exit(f"{name}: launches {strays} ended on a different screen than the rest. Nothing was recorded.")

    results = {
        name: {
            "command": apps[name],
            "first_paint_ms": summarize(measured, "first_paint_ms"),
            "ready_ms": summarize(measured, "ready_ms"),
            "runs": [{key: value for key, value in run.items() if key != "screen"} for run in measured],
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
