# GPUI Kit editor fixes

The workspace pins nine changed files on top of GPUI Kit **0.7.0**:

- Upstream tag: `v0.7.0`, commit `0c830f4d257e69fdd17200650533ab4ca9a40cc0`.
- Patched commit: [`e44d39248944fa18ae9636cc8987a8ccfada8c9b`](https://github.com/gregor-tokarev/gpui-kit/commit/e44d39248944fa18ae9636cc8987a8ccfada8c9b).
- Completion changes: [0001-current-frame-completion-position.patch](0001-current-frame-completion-position.patch).
- Query reuse: [0002-cache-syntax-queries.patch](0002-cache-syntax-queries.patch), applied after the first patch.
- Vim cursor support: [0003-vim-cursor-support.patch](0003-vim-cursor-support.patch), applied after the second patch.
- Idle caret: [0004-idle-caret.patch](0004-idle-caret.patch), applied after the third patch.

Inputs previously published their caret geometry during paint, after popups
had already chosen a position using the previous frame. Typing therefore
painted the menu at a stale position before a later redraw corrected it.

The patch publishes the existing geometry during input prepaint. The native
Scripts completion popup builds its element tree during deferred prepaint,
so layout, pointer hitboxes, and paint use that frame's caret position. The
app-owned variable popup uses the same timing. Both account for horizontal
scrolling already present in the caret coordinates.

The Scripts provider keeps its menu visible while TypeScript refreshes the
items. The patch retains the source/caret snapshot that produced each list:
Enter rejects stale ranges, and deferred mouse/keyboard insertion checks the
snapshot again. A pending-provider regression covers visibility, stale Enter,
stale clicks, and returning focus to the editor.

Cargo consumes the pinned Git source directly; the patch file is an audit copy,
not a build step. `gpui-kit-assets` shares the source because the component's
path dependency otherwise conflicts with its registry package's native `links`
identity. Assets and macros are unchanged. There is no framework version bump.

The completion patch includes a Base regression test for deferred overlay geometry.
Request Eagle's `tab_ui` tests check the first painted frame for Scripts, Body,
and URL completion, including the `pm.request` → `pm.request.` transition,
light/dark themes, 12/16/24 px interface sizes, and clicking the moved menu.

The second patch shares immutable compiled syntax queries across editors. Each
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

The third patch supports Vim's block cursor. `set_caret_hidden` hides only the
painted caret, and `scroll_to_offset` reveals the Vim cursor rather than an edge
of the Visual selection. `range_to_bounds` now returns `None` for offsets above
or below the laid-out lines instead of resolving them to the first visible line,
so the block disappears with its line. Base tests cover all three; Request
Eagle's Vim tests check the block in Normal, Visual, and Visual Line mode.

The fourth patch stops the caret from redrawing an idle window. Every blink
redraws the whole window, so a focused input kept an otherwise idle window
drawing twice a second for as long as it stayed focused. The caret now stops
blinking, visible, after ten seconds without input, as GTK does. Input or
focus starts it again. A Base test covers settling and restarting.

When upstream includes equivalent behavior, remove these overrides and this
directory together, retaining the app regression tests.
