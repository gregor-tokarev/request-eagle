# GPUI Kit and GPUI fixes

The workspace builds GPUI Kit **0.11.0** from a fork with fixes that are not
upstream yet. The same fork carries patched copies of GPUI's Linux and macOS
platform crates. GPUI Kit's own changes touch ten files:

- Upstream tag: `v0.11.0`, commit `e97fe438129c5e4ccab504f2e6fafd9a4e396bba`.
- Fork branch: [`request-eagle/patches-0.11.0`](https://github.com/gregor-tokarev/gpui-kit/tree/request-eagle/patches-0.11.0),
  pinned at [`b15e0e0b`](https://github.com/gregor-tokarev/gpui-kit/commit/b15e0e0b91a328e68830c90c6ec1b6261637e3d1).
- All changes: [`e97fe438...b15e0e0b`](https://github.com/gregor-tokarev/gpui-kit/compare/e97fe438129c5e4ccab504f2e6fafd9a4e396bba...b15e0e0b91a328e68830c90c6ec1b6261637e3d1).
- Stale completion snapshot guard: [`b158403f`](https://github.com/gregor-tokarev/gpui-kit/commit/b158403f29d0a5417c32892e93690442328c6dd9).
- Query reuse: [`12e3d97d`](https://github.com/gregor-tokarev/gpui-kit/commit/12e3d97dbdaed6a4bde8d2efc0d2f86fd8e8c4bf).
- Vim cursor support: [`4f9bc119`](https://github.com/gregor-tokarev/gpui-kit/commit/4f9bc1197769d80c533fb6d638b5591d91861eef).
- Visible search matches: [`e0898d73`](https://github.com/gregor-tokarev/gpui-kit/commit/e0898d73a35f1068815bdd99ed621d0053597727).
- Platform crates `gpui-pre-linux` and `gpui-pre-macos` 0.3.14:
  [`7007f323`](https://github.com/gregor-tokarev/gpui-kit/commit/7007f323151fb7e53ca7681ac7793e4844600e89)
  adds the published crates unchanged under `patches/`.
- Idle frame loops in `gpui-pre-linux` (X11) and `gpui-pre-macos` (macOS 13
  `CVDisplayLink` fallback):
  [`815a6ab8`](https://github.com/gregor-tokarev/gpui-kit/commit/815a6ab8da508a7b7a15f9712f17c12b893b2ff9)
  and [`b15e0e0b`](https://github.com/gregor-tokarev/gpui-kit/commit/b15e0e0b91a328e68830c90c6ec1b6261637e3d1).
- macOS windows without the opening animation in `gpui-pre-macos`:
  [`c0cb046d`](https://github.com/gregor-tokarev/gpui-kit/commit/c0cb046d8a828df270c6ab2d1f884ddb95e1b8f0).

The fork branch must stay on GitHub: Cargo fetches the pinned commit from it.

Current-frame completion positioning, idle caret timeout (`Theme::cursor_blink`),
select menus keeping their height when opening above the trigger, and macOS
`CAMetalDisplayLink` idle frame pausing moved upstream between 0.7.0 and 0.11.0
and were dropped from the fork.

The Scripts provider keeps its menu visible while TypeScript refreshes the
items. The stale completion guard retains the source/caret snapshot that
produced each list: Enter rejects stale ranges, and deferred mouse/keyboard
insertion checks the snapshot again.

`gpui-kit-assets` comes from the fork too, because the component's
path dependency otherwise conflicts with its registry package's native `links`
identity. Assets and macros are unchanged. There is no framework version bump.

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
so the block disappears with its line. Base tests cover all three.

The search match fix makes an editor's matches visible. They were painted in
the selection color, which themes keep faint, and every match except the
current one lost that color's saturation as well, so a search in the Pretty
response body looked like it had found nothing. They also disappeared while the
window was inactive. Editors now paint matches in the theme's yellow, every
match lightly and the current one strongly, whether or not the window is
active. The Raw response view paints its matches the same way.

The idle frame loop fix stops idle X11 windows and macOS 13 `CVDisplayLink`
windows from waking the app. On X11 a timer, and on macOS 13 the fallback
display link, asked GPUI for a frame on every display refresh, so a visible
window woke the app 60 to 120 times a second while nothing changed. Each
window's loop now stops 100 ms after GPUI last asked for a frame, and GPUI's
frame waker (plus `schedule_frame` on macOS 13) restarts it, as GPUI already
does on Wayland, macOS 14+ (`CAMetalDisplayLink`), and the web. On X11 the
waker pings the event loop, which restarts the timer, and a pending request
stands until the loop restarts; on macOS 13 it posts one step through the
window's dispatch source, which restarts the display link.

The opening animation fix shows a new macOS window at once. AppKit animated
every new normal window onto the screen for about 0.3 s, and the app took no
input until the animation ended, so Request Eagle answered its first key
press at 0.66 s after launch instead of 0.35 s. Normal windows now set
AppKit's animation behavior to none; pop-ups keep the utility animation.

When upstream includes equivalent behavior, remove these overrides and this
document together, retaining the app regression tests.
