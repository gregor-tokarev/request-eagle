# Startup benchmark

The landing page says Request Eagle is ready in 0.2 seconds and that Postman
takes 18 times as long. This page records where those figures come from.

## Result

Measured on 29 September 2026. Times are milliseconds from launch.

| App | First paint (median) | Ready (median) | Ready (fastest to slowest) |
| --- | ---: | ---: | ---: |
| Request Eagle 0.1.15 | 216 | 216 | 214 to 449 |
| Postman 12.30.3 | 598 | 3859 | 3815 to 3932 |

Postman's median is 17.9 times Request Eagle's. Comparing Postman's
fastest launch with Request Eagle's slowest still gives 8.5 times.

Request Eagle draws its complete window in its first frame, so its first paint
and ready times are the same. Postman shows an empty window, then a loading
indicator, and draws its request editor about three seconds later.

## Method

`scripts/startup-benchmark.py` launches each app on its own empty virtual
display and records that display at 60 frames per second.

- **First paint** is the first frame that shows a window.
- **Ready** is the frame of the last visible change before the screen stays
  unchanged for three seconds. Changes smaller than 300 pixels are ignored, so
  a blinking caret does not count.

Each app is launched once without being measured, then ten times. The apps take
turns, and each launch waits until the machine is idle.

A stalled app can look like a finished one. The script therefore rejects the
whole result unless every launch of an app ends on the same screen. An earlier
run, made while other work loaded the machine, recorded Postman as ready after
0.8 seconds because its loading indicator had stalled; that run was discarded
and led to this check.

A separate recording of every change during a Postman launch confirmed that
its ready time is the moment the request editor appears.

## Conditions

| | |
| --- | --- |
| Machine | AMD Ryzen 5 7430U, 12 threads, 30 GB memory |
| System | Ubuntu 26.04.1, Linux 7.0.0 |
| Display | Xvfb, 1280 × 800, both windows filling it |
| Request Eagle | 0.1.15 release build, two collections with eleven requests |
| Postman | 12.30.3 for Linux, used without an account, empty history |

## Limits

- **Linux, not macOS.** Request Eagle ships for macOS. Absolute times on a Mac
  will differ, and the ratio may too. Nothing here was measured on a Mac.
- **No GPU.** Xvfb has none, so both apps drew in software.
- **Warm starts.** Both apps were already in the file cache. A first launch
  after a restart is slower for both.
- **One machine, shared.** Other work ran on it between launches. Request
  Eagle's first launch took 449 ms, twice its usual time.
- **Postman without an account.** Signed in, Postman also loads workspaces and
  syncs, which this did not measure.
- **Ready is a visual measure.** It is the moment the window stops being drawn,
  not a measured response to input.

## Runs

| Run | Eagle first paint | Eagle ready | Postman first paint | Postman ready | Load |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 449 | 449 | 598 | 3898 | 1.90 |
| 2 | 214 | 214 | 567 | 3850 | 1.24 |
| 3 | 217 | 217 | 582 | 3815 | 0.99 |
| 4 | 216 | 216 | 615 | 3916 | 1.14 |
| 5 | 222 | 222 | 601 | 3884 | 1.17 |
| 6 | 214 | 214 | 598 | 3932 | 1.56 |
| 7 | 215 | 215 | 584 | 3850 | 1.47 |
| 8 | 233 | 233 | 600 | 3817 | 3.04 |
| 9 | 234 | 234 | 600 | 3868 | 1.47 |
| 10 | 216 | 216 | 567 | 3833 | 0.98 |

## Reproduce

Needs Linux, Xvfb, ffmpeg and Pillow. Download Postman for Linux, start it
once and choose to continue without an account.

```sh
cargo build -p request-eagle --release

scripts/startup-benchmark.py --runs 10 --out results.json \
    "eagle=env GPUI_X11_SCALE_FACTOR=1 target/release/request-eagle" \
    "postman=/path/to/Postman/Postman --no-sandbox"
```

When the figures change, update the race in `landing/index.html`: the
`data-ms` values, the visible times, the headline and the method text.
