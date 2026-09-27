# GPUI Kit completion positioning

The workspace pins a four-file fix on top of GPUI Kit **0.6.2**:

- Upstream tag: `v0.6.2`, commit `122c36f7be19ea0e179107c067b679efccb7d66a`.
- Patched commit: [`ff0dedf715f86c0ea04840301591d678060d5193`](https://github.com/gregor-tokarev/gpui-kit/commit/ff0dedf715f86c0ea04840301591d678060d5193).
- Reviewable source diff: [0001-current-frame-completion-position.patch](0001-current-frame-completion-position.patch).

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

The patch includes a Base regression test for deferred overlay geometry.
Request Eagle's `tab_ui` tests check the first painted frame for Scripts, Body,
and URL completion, including the `pm.request` → `pm.request.` transition,
light/dark themes, 12/16/24 px interface sizes, and clicking the moved menu.

When upstream includes equivalent behavior, remove these overrides and this
directory together, retaining the app regression tests.
