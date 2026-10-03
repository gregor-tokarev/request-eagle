# GPUI Kit editor fixes

The workspace builds GPUI Kit **0.7.0** from a fork with editor fixes that are
not upstream yet. The fork changes nine files:

- Upstream tag: `v0.7.0`, commit `0c830f4d257e69fdd17200650533ab4ca9a40cc0`.
- Fork branch: [`request-eagle/patches-0.7.0`](https://github.com/gregor-tokarev/gpui-kit/tree/request-eagle/patches-0.7.0),
  pinned at [`e44d3924`](https://github.com/gregor-tokarev/gpui-kit/commit/e44d39248944fa18ae9636cc8987a8ccfada8c9b).
- All changes: [`v0.7.0...request-eagle/patches-0.7.0`](https://github.com/gregor-tokarev/gpui-kit/compare/v0.7.0...request-eagle/patches-0.7.0).
- Completion changes: [`4eb43a09`](https://github.com/gregor-tokarev/gpui-kit/commit/4eb43a09ed12cedb409a826d055678164a28bae8), [`9b588e02`](https://github.com/gregor-tokarev/gpui-kit/commit/9b588e027112fadcc00ccd979e01437402e8a2e0) and [`6aa23423`](https://github.com/gregor-tokarev/gpui-kit/commit/6aa2342323f92d4af2b026b294da7422e8d98389).
- Query reuse: [`55ed1bb5`](https://github.com/gregor-tokarev/gpui-kit/commit/55ed1bb5b4afa407ca6fb195003820ae0327c309).
- Vim cursor support: [`cc673b8f`](https://github.com/gregor-tokarev/gpui-kit/commit/cc673b8f9c51aba0cd912078f52699032c44245c) and [`e9affedd`](https://github.com/gregor-tokarev/gpui-kit/commit/e9affedd20e6bc04f7a9412f0e6491fd10672ce3).
- Idle caret: [`e44d3924`](https://github.com/gregor-tokarev/gpui-kit/commit/e44d39248944fa18ae9636cc8987a8ccfada8c9b).

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

When upstream includes equivalent behavior, remove these overrides and this
document together, retaining the app regression tests.
