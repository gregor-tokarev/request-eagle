# GPUI Kit editor fixes

The workspace pins seven changed files on top of GPUI Kit **0.6.2**:

- Upstream tag: `v0.6.2`, commit `122c36f7be19ea0e179107c067b679efccb7d66a`.
- Patched commit: [`45c222ca2a43bcaa67a4c43f0cba62b1e2e74461`](https://github.com/gregor-tokarev/gpui-kit/commit/45c222ca2a43bcaa67a4c43f0cba62b1e2e74461).
- Completion changes: [0001-current-frame-completion-position.patch](0001-current-frame-completion-position.patch).
- Query reuse: [0002-cache-syntax-queries.patch](0002-cache-syntax-queries.patch), applied after the first patch.

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
alongside preference loading and awaits both before opening its first workspace.
This avoids compiling the same JavaScript queries during each editor's first
redraw, including when opening an empty Scripts editor immediately after launch.

When upstream includes equivalent behavior, remove these overrides and this
directory together, retaining the app regression tests.
