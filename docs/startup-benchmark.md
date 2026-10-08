# Startup benchmark

The landing page says Request Eagle answers a keystroke 0.36 seconds after
launch and that Postman takes 12 times as long. This page records where those
figures come from. An earlier measurement on Linux, of when each window stops
being drawn, follows it.

## Time to first interaction on macOS

Measured on 8 October 2026. Times are milliseconds from launch.

| App | Answers a keystroke (median) | Fastest to slowest | Window fully drawn (median) |
| --- | ---: | ---: | ---: |
| Request Eagle 0.1.28 | 359 | 347 to 372 | 340 |
| Postman 12.31.3 | 4331 | 4192 to 4392 | 2680 |

Postman's median is 12.1 times Request Eagle's. Comparing Postman's fastest
launch with Request Eagle's slowest still gives 11.3 times.

Request Eagle draws its complete window in its first frame, and that frame
already shows the answer to a key pressed before the window appeared. Postman
draws its interface after about 2.7 seconds and answers its first keystroke
about 1.6 seconds later.

### Method

Each app is opened with `open -a`, as the Finder opens it, while the screen is
recorded at 60 frames per second with ffmpeg's `avfoundation` input. Every 50 ms
from the launch, while the app is frontmost, a shortcut that opens an overlay
and saves nothing is pressed:

- **Request Eagle:** Cmd+K, which opens the command palette.
- **Postman:** Cmd+comma, which opens its settings. Postman has no Cmd+K.

A calibration launch of each app first records the screen with and without its
overlay. A launch's time is the capture time of the first frame in which 40%
of the overlay's pixels match it. Because keys are pressed every 50 ms, a time
can be up to 50 ms late.

Each app is launched once without being measured, then five times. The apps
take turns, other apps are hidden, and each launch waits until the machine is
idle.

Earlier releases answered later, and the app takes no input while macOS
animates its window. 0.1.26 opened its window maximized, which macOS animates
to full size, and answered at 693 ms (measured with macOS's window animations
switched off). 0.1.27 opened it at full size, and answered at 662 ms because of
macOS's window-opening animation. 0.1.28 shows the window without animation.

### Conditions

| | |
| --- | --- |
| Machine | MacBook Pro, Apple M5 Pro, 18 cores, 48 GB memory |
| System | macOS 26.5.2, default settings |
| Display | 3440 × 1440 at 1×, both windows filling it |
| Request Eagle | 0.1.28 nightly, two tabs, three collections |
| Postman | 12.31.3, signed in, a workspace with fifteen collections |

### Limits

- **Different shortcuts.** The two overlays are different features. Both are
  the first visible answer to a key press.
- **Signed in.** Postman loads the account's workspace. Without an account its
  times may differ.
- **Warm starts.** Both apps were already in the file cache.
- **One machine.** Other apps were running, hidden.

### Runs

| Run | Request Eagle | Postman |
| ---: | ---: | ---: |
| 1 | 347 | 4331 |
| 2 | 359 | 4382 |
| 3 | 372 | 4318 |
| 4 | 359 | 4392 |
| 5 | 350 | 4192 |

## Window drawn on Linux

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

### Method

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

### Conditions

| | |
| --- | --- |
| Machine | AMD Ryzen 5 7430U, 12 threads, 30 GB memory |
| System | Ubuntu 26.04.1, Linux 7.0.0 |
| Display | Xvfb, 1280 × 800, both windows filling it |
| Request Eagle | 0.1.15 release build, two collections with eleven requests |
| Postman | 12.30.3 for Linux, used without an account, empty history |

### Limits

- **Linux, not macOS.** Request Eagle ships for macOS. Absolute times on a Mac
  will differ, and the ratio may too. Nothing in this section was measured on
  a Mac.
- **No GPU.** Xvfb has none, so both apps drew in software.
- **Warm starts.** Both apps were already in the file cache. A first launch
  after a restart is slower for both.
- **One machine, shared.** Other work ran on it between launches. Request
  Eagle's first launch took 449 ms, twice its usual time.
- **Postman without an account.** Signed in, Postman also loads workspaces and
  syncs, which this did not measure.
- **Ready is a visual measure.** It is the moment the window stops being drawn,
  not a measured response to input.

### Runs

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

### Reproduce

Needs Linux, Xvfb, ffmpeg and Pillow. Download Postman for Linux, start it
once and choose to continue without an account.

```sh
cargo build -p request-eagle --release

scripts/startup-benchmark.py --runs 10 --out results.json \
    "eagle=env GPUI_X11_SCALE_FACTOR=1 target/release/request-eagle" \
    "postman=/path/to/Postman/Postman --no-sandbox"
```

When the figures change, update the race in `landing/src/pages/index.astro`:
the `data-ms` values, the visible times, the headline and the method text.
