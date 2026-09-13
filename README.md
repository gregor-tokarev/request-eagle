<p align="center">
  <img src=".github/icon-eagle-v2.png" width="128" alt="Request Eagle icon">
</p>

<h1 align="center">Request Eagle</h1>

<p align="center">zero-friction API client</p>

## Development

Request Eagle uses Longbridge's `gpui-kit` 0.6.0. Import GPUI types from `gpui_kit`
and styled controls from `gpui_kit::component`. The workspace pins the kit
release; individual crates enable `component`, `assets`, and `test-support`
as needed. The HTTP client uses the matching `gpui-pre-reqwest-client` release
because the kit does not re-export it.

Run `cargo test --workspace --locked` to test the workspace, or `make run` to
build and launch the macOS app bundle.

The sidebar loads collections from `~/.request-eagle/collections`. Set
`REQUEST_EAGLE_COLLECTIONS_DIR` to use another directory. Filtering, collapsing,
request selection, and keyboard navigation use a virtual list that draws only
the visible rows.

The collection tree receives keyboard focus when the workspace opens. Up/Down
select rows, Left/Right collapse or expand folders and move to parents or children,
and Home/End select the first or last row. Enter/Space toggle the selected folder.
Shift-Tab returns to the filter; Tab moves from the filter into the tree. From
the filter, Down/Enter select the first result row and Up selects the last.

Right-click any collection, folder, or request for Rename, Open in Finder, Copy Path,
and Delete. Copy Path copies the backing file or directory path. Double-click a row to edit its name; Enter saves, while Escape or moving
focus away cancels. Delete and Backspace show an inline Yes/No confirmation in
the selected row. Yes deletes; No or Escape cancels. Moving selection or focusing
the filter also cancels. Backspace only acts when the tree has focus.
Deletion removes the backing file or directory, including a directory's contents.
Request renames update the TOML name while preserving comments and custom fields.
Collection and folder renames move their directories and update descendant paths.

Search builds a [suffix-array index](https://docs.rs/suffix/1.3.0/suffix/struct.SuffixTable.html)
when collections load. It looks up case-insensitive substrings in collection and
folder names, request names, methods, and paths without scanning every request.
Result assembly visits matching rows, their ancestors, and matched folders'
descendants. Clearing the search restores cached browsing rows and collapse state.

`make dev` watches project files and rebuilds and restarts an optimized release
build named **Request Eagle (Dev)**. GPUI's detailed frame monitor appears in
the upper-right corner. It shows current CPU draw time, `1%` for p99, `10%` for
p90, maximum draw time, and frame count. Percentiles use the latest 1,000 draws;
the 120 fps frame budget is 8.33 ms. These are CPU draw times, not displayed FPS
or GPU timings. Stop the app and watcher with Ctrl-C. Normal release builds do
not enable the monitor.

To benchmark CPU drawing and scrolling with 100,000 generated requests, run:

```sh
REQUEST_EAGLE_BENCH_PAGE='Workspace 100000 requests' \
  cargo test -p workspace --release --locked pages_render_benchmark \
  -- --ignored --nocapture --test-threads=1
```

The benchmark creates temporary collections and removes them after loading.
It excludes collection loading, GPU presentation, and the native Root wrapper.

To measure search latency separately, including result assembly, run:

```sh
cargo test -p workspace --release --locked collection_search_benchmark \
  -- --ignored --nocapture --test-threads=1
```

This compares the index with the previous full scan on 1,000, 10,000, and 100,000
requests. It includes short common queries, selective paths, misses, and folder
matches, and reports index build time and storage. Broad queries still take time
proportional to their matches and the rows they return.

On an Apple M5 Pro release build with 100,000 generated requests, median CPU
search times, including result assembly, were:

| Query | Previous scan | Indexed search |
| --- | ---: | ---: |
| `get` | 0.864 ms | 0.561 ms |
| `/resources/99` | 0.472 ms | 0.0083 ms |
| `/resources/99999` | 0.492 ms | 0.0004 ms |

Building the tree and index took 97 ms once; the retained index payload was
35.5 MiB. These timings exclude UI scheduling and GPU presentation.

## Settings

Application preferences live in `~/.request-eagle/preferences.json`, grouped by
section, currently `appearance`. The `preferences` crate owns defaults, loading,
and atomic saves through `preferences::update`. Feature crates apply
the shared values to their UI; the theme crate observes preference changes and
updates colors and typography. Keybindings keep their separate service and file.

Appearance offers 11 theme families, each with one light and one dark variant.
Selecting either variant selects its partner, so switching Light, Dark, or System
keeps the same family. On upgrade, the active saved family takes precedence.
Removed themes use the remaining saved partner's family, or Ayu if neither is
available. Catppuccin Frappe and Macchiato move to Latte/Mocha.

Open Settings with **⌘,**, the status bar settings button, or **Request Eagle → Settings…**.
General shows the installed version and update status. Use **Check for updates**
to check manually, then **Install and relaunch** when a new version is available.
**Request Eagle → About Request Eagle** opens General; **Check for Updates…** opens General
and starts a check. Automatic checks update the status quietly without opening Settings.

The Keybindings page lists the settings and workspace shortcuts. Search by command
name, or click the keyboard button in the search bar and press a shortcut to find
its commands. Click outside the search bar to stop capturing keys, or use the
clear button to show all commands and return to text search. Click a shortcut in a row to record a replacement.
The recorder stays in the shortcut's column and captures every key, including
Escape, Enter, Tab, and Backspace. Use the buttons beside it to save, cancel,
or remove the shortcut. Each row also has a Remove button that works without
opening the recorder. You can reset individual commands or all commands to their defaults.

Changes take effect immediately and save to `~/.request-eagle/keybindings.json`.
If a saved shortcut conflicts with another binding at startup, the first registered
binding stays active and the conflicting command appears unassigned with an error
in Settings. Record a replacement or reset it to resolve the conflict. Startup
leaves the saved file unchanged.

New application commands should use `keybindings_service::register` to appear in
Settings with their label, description, category, and default shortcut.

## macOS releases

Push a semantic version tag such as `v0.1.0` to run the macOS release
workflow. The version in `crates/request-eagle/Cargo.toml` must match the tag.

The workflow builds an Apple Silicon (`arm64`) app, signs it with a Developer ID
Application certificate, enables the hardened runtime, notarizes the app and
DMG with Apple, staples the notarization tickets, and publishes the ZIP, DMG,
update manifest, and SHA-256 checksums to a GitHub release.

Request Eagle checks the latest GitHub release when an installed app starts. Users can
also choose **Request Eagle → Check for Updates…**. Before replacing the app, the
updater verifies the archive checksum, Apple Developer ID signature, expected
Team ID, and Gatekeeper assessment.
