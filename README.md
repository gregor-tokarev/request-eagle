<p align="center">
  <img src=".github/icon-eagle-v2.png" width="128" alt="Request Eagle icon">
</p>

<h1 align="center">Request Eagle</h1>

<p align="center">A native API client for macOS.</p>

<p align="center">
  <a href="https://github.com/gregor-tokarev/request-eagle/releases/latest">Download for macOS</a>
  ·
  <a href="https://github.com/gregor-tokarev/request-eagle/issues">Feedback &amp; ideas</a>
</p>

Request Eagle is a Postman alternative built in Rust with GPUI. It keeps your
request collections in local, readable files.

- Requests stored as TOML, ready to keep in version control.
- Collections and nested folders to organize your APIs.
- Search across request names, methods, and URLs.
- Light and dark themes with customizable keyboard shortcuts.

The project is in early development. macOS builds are available for Apple Silicon.

## Build from source

On macOS, install Rust and the Xcode command-line tools. On Linux, install
Rust and the native build dependencies for GPUI (a C/C++ toolchain, CMake, `make`,
`pkg-config`, and the development libraries for Fontconfig, FreeType, X11,
Wayland, and xkbcommon).

Then build and run:

```sh
git clone https://github.com/gregor-tokarev/request-eagle.git
cd request-eagle
make run
```

For development on macOS or Linux, use `make dev`. It builds in release mode
with GPUI's frame monitor and rebuilds and restarts the app when source files
change. Press Ctrl-C to stop. Linux runs the native executable directly;
macOS creates a development app bundle.

On Linux, `make run` and `make dev` use the terminal's `WAYLAND_DISPLAY` or
`DISPLAY`. If neither is set (for example, in a remote terminal), the launcher
uses the single Wayland socket in `XDG_RUNTIME_DIR`, defaulting to
`/run/user/$(id -u)`. If no display can be selected unambiguously, it reports an
error instead of starting invisibly. Set `WAYLAND_DISPLAY` and
`XDG_RUNTIME_DIR`, or `DISPLAY` for X11, explicitly to choose a desktop session.

Application shortcuts use Command on macOS and Ctrl on Linux: Ctrl+T opens a
tab, Ctrl+W closes a tab, and Ctrl+1–9 selects a tab (9 selects the last tab).
On Linux, Ctrl+Shift+[ and Ctrl+Shift+] cycle tabs. Super remains available for
desktop shortcuts such as Hyprland's floating, workspace, and close-window
commands.

To measure Settings → Keybindings page switches with the frame monitor enabled:

```sh
REQUEST_EAGLE_BENCH_PAGE=Keybindings REQUEST_EAGLE_BENCH_TRANSITIONS=1 \
REQUEST_EAGLE_BENCH_OVERLAY=1 REQUEST_EAGLE_BENCH_SAMPLES=200 \
cargo test -p workspace --features dev-profiler --release pages_render_benchmark \
  -- --ignored --nocapture --test-threads=1
```

This reports CPU draw percentiles at three window sizes. It excludes native window
integration and GPU presentation; also check the frame monitor in `make dev` when
assessing the 8.33 ms budget for 120 fps.
