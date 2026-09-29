# Startup benchmark

The landing page says Request Eagle is ready in 0.2 seconds and that Postman
takes 17 times as long. This page records where those figures come from.

## Result

Measured on 29 September 2026. Times are milliseconds from launch.

| App | First paint (median) | Ready (median) | Ready (fastest to slowest) |
| --- | ---: | ---: | ---: |
| Request Eagle 0.1.15 | 218 | 218 | 216 to 282 |
| Postman 12.30.3 | 582 | 3758 | 3716 to 4936 |

Postman's median is 17.2 times Request Eagle's. Comparing Postman's
fastest launch with Request Eagle's slowest still gives 13.2 times.

Request Eagle draws its complete window in its first frame, so its first paint
and ready times are the same. Postman shows an empty window, then a loading
indicator, and draws its request editor about three seconds later.

## Method

`scripts/startup-benchmark.py` launches each app on its own empty virtual
display and records that display at 60 frames per second.

- **First paint** is the first frame that shows a window.
- **Ready** is the frame of the last visible change before the screen stays
  unchanged for two seconds. Changes smaller than 300 pixels are ignored, so a
  blinking caret does not count.

Each app is launched once without being measured, then ten times. The apps take
turns, so background load affects both alike.

A separate recording of every change during a Postman launch confirmed that
its ready time is the moment the request editor appears, not a later cosmetic
update.

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
- **One machine.** Background load rose during runs 5 to 8, which shows in the
  slowest times.
- **Postman without an account.** Signed in, Postman also loads workspaces and
  syncs, which this did not measure.
- **Ready is a visual measure.** It is the moment the window stops being drawn,
  not a measured response to input.

## Runs

| Run | Eagle first paint | Eagle ready | Postman first paint | Postman ready | Load |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 217 | 217 | 582 | 3716 | 0.56 |
| 2 | 216 | 216 | 583 | 3718 | 0.38 |
| 3 | 218 | 218 | 582 | 3766 | 0.39 |
| 4 | 216 | 216 | 567 | 3718 | 0.44 |
| 5 | 218 | 218 | 600 | 3734 | 3.70 |
| 6 | 282 | 282 | 1267 | 4936 | 6.80 |
| 7 | 250 | 250 | 583 | 3800 | 5.58 |
| 8 | 234 | 234 | 565 | 3750 | 3.17 |
| 9 | 217 | 217 | 651 | 3887 | 2.12 |
| 10 | 216 | 216 | 566 | 3767 | 1.65 |

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
