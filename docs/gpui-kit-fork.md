# GPUI Kit and GPUI fixes

The workspace builds GPUI Kit **0.7.0** from a fork with fixes that are not
upstream yet. The same fork carries patched copies of GPUI's Linux and macOS
platform crates. GPUI Kit's own changes touch ten files:

- Upstream tag: `v0.7.0`, commit `0c830f4d257e69fdd17200650533ab4ca9a40cc0`.
- Fork branch: [`request-eagle/patches-0.7.0`](https://github.com/gregor-tokarev/gpui-kit/tree/request-eagle/patches-0.7.0),
  pinned at [`81c79c8f`](https://github.com/gregor-tokarev/gpui-kit/commit/81c79c8fb85b7b7b1aeb449fa9a7b466762c5fdc).
- All changes: [`0c830f4d...81c79c8f`](https://github.com/gregor-tokarev/gpui-kit/compare/0c830f4d257e69fdd17200650533ab4ca9a40cc0...81c79c8fb85b7b7b1aeb449fa9a7b466762c5fdc).
- Completion changes: [`4eb43a09`](https://github.com/gregor-tokarev/gpui-kit/commit/4eb43a09ed12cedb409a826d055678164a28bae8), [`9b588e02`](https://github.com/gregor-tokarev/gpui-kit/commit/9b588e027112fadcc00ccd979e01437402e8a2e0) and [`6aa23423`](https://github.com/gregor-tokarev/gpui-kit/commit/6aa2342323f92d4af2b026b294da7422e8d98389).
- Query reuse: [`55ed1bb5`](https://github.com/gregor-tokarev/gpui-kit/commit/55ed1bb5b4afa407ca6fb195003820ae0327c309).
- Vim cursor support: [`cc673b8f`](https://github.com/gregor-tokarev/gpui-kit/commit/cc673b8f9c51aba0cd912078f52699032c44245c) and [`e9affedd`](https://github.com/gregor-tokarev/gpui-kit/commit/e9affedd20e6bc04f7a9412f0e6491fd10672ce3).
- Idle caret: [`e44d3924`](https://github.com/gregor-tokarev/gpui-kit/commit/e44d39248944fa18ae9636cc8987a8ccfada8c9b).
- Spinner frame rate: [`81c79c8f`](https://github.com/gregor-tokarev/gpui-kit/commit/81c79c8fb85b7b7b1aeb449fa9a7b466762c5fdc).
- Idle frame loops in `gpui-pre-linux` and `gpui-pre-macos` 0.3.7:
  [`291bd0c3`](https://github.com/gregor-tokarev/gpui-kit/commit/291bd0c3ca2181ce88baeea2320185568d7a27fe)
  adds the published crates unchanged under `patches/`, and
  [`c113cdd1`](https://github.com/gregor-tokarev/gpui-kit/commit/c113cdd18491183a8ad00d869ebf775625667e90)
  patches them.

The fork branch must stay on GitHub: Cargo fetches the pinned commit from it.

Inputs previously published their caret geometry during paint, after popups
had already chosen a position using the previous frame. Typing therefore
painted the menu at a stale position before a later redraw corrected it.

The fix publishes the existing geometry during input prepaint. The native
Scripts completion popup builds its element tree during deferred prepaint,
so layout, pointer hitboxes, and paint use that frame's caret position. The
app-owned variable popup uses the same timing. Both account for horizontal
scrolling already present in the caret coordinates.

The Scripts provider keeps its menu visible while TypeScript refreshes the
items. The fix retains the source/caret snapshot that produced each list:
Enter rejects stale ranges, and deferred mouse/keyboard insertion checks the
snapshot again. A pending-provider regression covers visibility, stale Enter,
stale clicks, and returning focus to the editor.

`gpui-kit-assets` comes from the fork too, because the component's
path dependency otherwise conflicts with its registry package's native `links`
identity. Assets and macros are unchanged. There is no framework version bump.

The completion fix includes a Base regression test for deferred overlay geometry.
Request Eagle's `tab_ui` tests check the first painted frame for Scripts, Body,
and URL completion, including the `pm.request` → `pm.request.` transition,
light/dark themes, 12/16/24 px interface sizes, and clicking the moved menu.

The query reuse fix shares immutable compiled syntax queries across editors. Each
editor still owns its parser, source, and syntax tree. Cache keys include the
actual grammar and all query sources, so replacing a grammar or its queries
cannot reuse obsolete captures. Compilation happens outside the cache lock.
Tests cover query reuse, background warm-up, replacements, failed compilation,
and independent document trees; the existing highlighting/injection tests also
pass with shared queries.

Request Eagle compiles JSON and JavaScript queries on the background executor
at launch. This avoids compiling the same JavaScript queries during each
editor's first redraw. The window does not wait for them: creating it takes
longer than compiling them, and no editor exists until a Body or Scripts
section is shown.

The Vim cursor fix supports Vim's block cursor. `set_caret_hidden` hides only the
painted caret, and `scroll_to_offset` reveals the Vim cursor rather than an edge
of the Visual selection. `range_to_bounds` now returns `None` for offsets above
or below the laid-out lines instead of resolving them to the first visible line,
so the block disappears with its line. Base tests cover all three; Request
Eagle's Vim tests check the block in Normal, Visual, and Visual Line mode.

The idle caret fix stops the caret from redrawing an idle window. Every blink
redraws the whole window, so a focused input kept an otherwise idle window
drawing twice a second for as long as it stayed focused. The caret now stops
blinking, visible, after ten seconds without input, as GTK does. Input or
focus starts it again. A Base test covers settling and restarting.

The spinner fix lets spinners turn 12 times a second instead of on every
display refresh. Each step redraws the window, and a spinner can show for as
long as a slow request takes. The default icon becomes `LoaderCircle`, since
the eight spokes of `Loader` look the same after a coarse step.

The idle frame loop fix stops an idle window from waking the app. On X11 a
timer, and on macOS the display link, asked GPUI for a frame on every display
refresh, so a visible window woke the app 60 to 120 times a second while
nothing changed. Each window's loop now stops 100 ms after GPUI last asked for
a frame, and GPUI's frame waker restarts it, as GPUI already does on Wayland
and the web. On X11 the waker pings the event loop, which restarts the timer;
on macOS it posts one step through the window's dispatch source, which
restarts the display link.

When upstream includes equivalent behavior, remove these overrides and this
document together, retaining the app regression tests.
