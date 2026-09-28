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

## Optional CLI for agents

The separate **Request Eagle CLI** lets agents manage saved collections, edit
requests, run them, and change settings using JSON commands. It works without
the desktop app and is not included in the app bundle. **Settings → General**
links to the optional download and installation instructions. See the
[CLI guide](docs/cli.md) for installation, command discovery, and examples.

## Releases

The `Daily patch release` workflow checks `main` every day at 06:17 UTC. If there
are commits after the latest stable release tag, it increments the app's patch
version in `Cargo.toml` and `Cargo.lock`, commits the change, and pushes a `vX.Y.Z`
tag. It then calls the macOS workflow to build, sign, notarize, and publish the
release. With no new commits, it skips publishing unless the latest tag still
needs a release after a failed run. It can also be started manually from Actions.

The workflow uses `GITHUB_TOKEN` with `contents: write` and the existing Apple
signing secrets. Repository rules must allow this token to push release commits
to `main` and create tags. The macOS workflow is called directly because tag
pushes made with `GITHUB_TOKEN` do not trigger another workflow.

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
with the [GPUI Kit FPS monitor](https://gpui-kit.com/versions/main/docs/fps/)
and rebuilds and restarts the app when source files change. Press Ctrl-C to stop.
Linux runs the native executable directly;
macOS creates a development app bundle.

The application owns the monitor, enabled by `request-eagle/dev-profiler`, and
keeps it visible across workspace and settings. Click it to collapse or expand it;
right-click to switch between estimated maximum FPS and actual FPS. Normal builds
omit the monitor.

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

Command+Enter (Ctrl+Enter on Linux/Windows) sends the active request from its URL,
fields, JSON editor, or response. The shortcut can be changed in Settings →
Keybindings. Repeating it while a request is running does not cancel that request.

Settings → General → Editor → Vim mode enables Vim keybindings in request body
and pre-request/post-response script editors. It is off by default, saved across
restarts, and applies to open tabs immediately. The editor shows its current mode.
Use `i`/`a` to insert and Escape or Ctrl+`[` to return to Normal mode. Supported
bindings include `h j k l`, `w b e` (and `W B E`), `0 ^ $`, `gg G`, counts,
`d c y` with motions or doubled for lines, `x`, `p P`, `u`/Ctrl+R, `v V`,
`I A o O`, and `/`/`n N` through the editor's search. This is a core Vim binding
set; Ex commands, macros, text objects, and dot-repeat are not implemented.
Operators also accept `/`, `n`, and `N` as motions.
For `/` searches, Enter accepts the match (including a preceding count), and
Escape restores the original position or Visual selection. Visual `u`/`U`
changes case, and Visual `D C Y S X` operates on the selected lines.
Dragging to select text in Normal mode enters Visual mode, so motions and
operators act on the highlighted text. Insert mode keeps native selection editing.

Type `{{` in a request URL, query key/value, header key/value, or JSON body to
choose a variable. Continue typing to filter; use Up/Down and Enter or Tab to
insert, click a suggestion, or press Escape to dismiss. Completion replaces the
reference around the caret, keeping surrounding text and existing closing braces.

- `{{name}}` reads the collection's `environment.toml`. The file contains flat
  string entries, for example `base_url = "https://example.com"`. Suggestions read
  names when completion opens; Send reads the latest values. Requests outside a
  collection can use generated variables.
- `{{$guid}}`, `{{$isoTimestamp}}`, `{{$timestamp}}`, `{{$randomInt}}`,
  `{{$randomBoolean}}`, `{{$randomAlphaNumeric}}`, and `{{$randomEmail}}` generate
  values when sending. Repeated occurrences of a generated name share one value
  per send; the next send generates fresh values. The timestamp formats, UUID v4,
  integer range (0–1000), boolean, and single alphanumeric character follow
  [Postman's dynamic variables](https://learning.postman.com/docs/tests-and-scripts/write-scripts/variables-list/).
  Random email addresses use the `example.com` domain.
- `{{!name}}` sends the literal text `{{name}}`, without looking up a variable.
  For example, `{"template":"Hello {{!customer}}"}` sends a Mustache template
  unchanged as `{"template":"Hello {{customer}}"}`.

Send resolves a copy of the request, leaving the request draft and saved file
unchanged. Unknown or unclosed variables stop execution with an error. Values
are substituted literally, without recursive expansion or automatic JSON
escaping; quote string placeholders in JSON and escape special characters in
their values. Disabled rows, URL fragments, and bodies excluded by GET/HEAD are
not resolved. An invalid environment file blocks only requests that need its
values. Environment files are read-only in the application. Scripts can override
their values for the workspace session using `pm.environment`; these changes
are shared with other requests in that collection and never written to disk.

Request scripts support asynchronous HTTP calls, session environment values,
SHA-256/HMAC signing, Base64 helpers, JSON Schema checks, and conditional request
skipping. Open **Scripts**, choose **Pre-request** or **Post-response**, and use
the **Snippets** menu or API completions to get started. See the
[scripting API and workflow examples](docs/scripting.md).

Response header and cookie names and values support text selection and copying.
Hover over status, response time, or size for selectable details. Timings include
preparation, waiting for headers, downloading the body, and formatting. Connection
phases are included in waiting; the client does not expose separate DNS/TCP/TLS
timings. Header byte counts are text-size estimates, and request counts exclude
headers added automatically by the transport.

To measure Settings → Keybindings page switches:

```sh
REQUEST_EAGLE_BENCH_PAGE=Keybindings REQUEST_EAGLE_BENCH_TRANSITIONS=1 \
REQUEST_EAGLE_BENCH_SAMPLES=200 \
cargo test -p workspace --release pages_render_benchmark \
  -- --ignored --nocapture --test-threads=1
```

This reports CPU draw percentiles at three window sizes. It excludes the
application's FPS monitor, native window integration and GPU presentation; also
check the monitor in `make dev` when assessing the 8.33 ms budget for 120 fps.

To check tab creation, Ctrl+} switching, and horizontal scrolling with 100,
1,000, and 10,000 open tabs against the 120 fps CPU budget:

```sh
cargo test -p workspace --release tabs_interaction_benchmark \
  -- --ignored --nocapture --test-threads=1
```

This includes event dispatch, notifications, GPUI's Root wrapper, CPU drawing,
and element cleanup at the same three window sizes. It excludes the application's
FPS monitor. It reports mean, p95, p99, maximum, and the number of frames over
8.33 ms, and fails
if p99 exceeds that budget. Run serially on an otherwise idle machine. The first
20 interactions warm up each case; `REQUEST_EAGLE_BENCH_SAMPLES` controls the
number measured (default 120). Native input, font rendering, GPU work, and display
refresh still need verification in `make dev`; this CPU test does not prove a
120 Hz presentation rate.

To check the response UI inside the full workspace with 1,000 collection
requests, a large JSON body, 161 headers (including 32 cookies), scrolling,
and selection inside the timing and size overlays:

```sh
cargo test -p workspace --release response_interaction_benchmark \
  -- --ignored --nocapture --test-threads=1
```

This uses the same 8.33 ms p99 CPU budget and window sizes. Run it serially without
competing builds or benchmarks; native GPU presentation must be checked separately.

To measure Vim cursor movement in the Body and Scripts editors:

```sh
REQUEST_EAGLE_BENCH_VIM=1 cargo test -p tab_ui --release vim_cursor_benchmark \
  -- --ignored --nocapture --test-threads=1
```

Use `REQUEST_EAGLE_BENCH_VIM=0` for the disabled comparison and
`REQUEST_EAGLE_BENCH_EDITORS=100` to retain 100 additional inactive editor states.
Each case warms up
for 120 movements and measures 1,000 more, reporting CPU interaction percentiles
and allocated bytes. This includes dispatch, effects, Root, drawing and cleanup;
it excludes the FPS monitor, native font rendering and GPU presentation. Run on
an otherwise idle machine and check the native monitor separately.

The response viewer displays and searches the complete body, with wrapping on
by default. Responses over 256 KiB, or containing a line over 32 KiB, use an
app-owned read-only viewport. These thresholds choose the renderer; they do not
limit the displayed text. Smaller responses keep the standard syntax editor.

The custom viewport uses GPUI's line-breaking helper to index row starts as byte
offsets. It shapes and paints only visible rows, including when the response is
one long minified line. Search scans the existing response string directly and
highlights the current match. It reuses match offsets across keystrokes.
Raw/JSON switching, selection, copying and the wrapping toggle remain available.
No dependency patches or vendored crates are required.

The normal test suite checks full-body search beyond 1 MiB and allocation traffic
while scrolling responses over 10 MiB at two window widths:

```sh
cargo test -p tab_ui --release response_view -- --nocapture
```

The allocation tests count Rust allocation traffic, not retained or GPU memory.
Native memory checks must also exercise response loading, Raw/JSON switching,
scrolling and searching near the end. Memory still scales with the stored body,
formatted JSON and search results; viewport rendering does not shape offscreen
text. Large responses currently use plain text without syntax coloring.

For standard-editor HTML measurements, save the decoded HTTP body locally and run:

```sh
REQUEST_EAGLE_HTML_FIXTURE=/tmp/page.html cargo test -p tab_ui --release \
  standard_html_editor_benchmark -- --ignored --nocapture --test-threads=1
```

This deliberately uses the standard editor to measure wrapping, scrolling,
search and allocation traffic at 1024 and 640 px. It excludes native GPU
presentation and retained memory, which must be checked in the app.
