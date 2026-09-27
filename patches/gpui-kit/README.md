# GPUI Kit completion positioning

The workspace pins a two-file fix on top of GPUI Kit **0.6.2**:

- Upstream tag: `v0.6.2`, commit `122c36f7be19ea0e179107c067b679efccb7d66a`.
- Patched commit: [`aae12bf4dbf4c70e35329a7715abed9f33731fe7`](https://github.com/gregor-tokarev/gpui-kit/commit/aae12bf4dbf4c70e35329a7715abed9f33731fe7).
- Reviewable source diff: [0001-current-frame-completion-position.patch](0001-current-frame-completion-position.patch).

Inputs previously published their caret geometry during paint, after popups
had already chosen a position using the previous frame. Typing therefore
painted the menu at a stale position before a later redraw corrected it.

The patch publishes the existing geometry during input prepaint. The native
Scripts completion popup builds its element tree during deferred prepaint,
so layout, pointer hitboxes, and paint use that frame's caret position. The
app-owned variable popup uses the same timing. Both account for horizontal
scrolling already present in the caret coordinates.

Cargo consumes the pinned Git source directly; the patch file is an audit copy,
not a build step. `gpui-kit-assets` shares the source because the component's
path dependency otherwise conflicts with its registry package's native `links`
identity. Assets and macros are unchanged. There is no framework version bump.

The patch includes a Base regression test for deferred overlay geometry.
Request Eagle's `tab_ui` tests check the first painted frame for Scripts, Body,
and URL completion, including the `pm.request` → `pm.request.` transition,
light/dark themes, 12/16/24 px interface sizes, and clicking the moved menu.

When upstream includes equivalent behavior, remove these overrides and this
directory together, retaining the app regression tests.
