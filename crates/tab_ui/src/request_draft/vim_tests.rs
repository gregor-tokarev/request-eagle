use gpui_kit::{
    AppContext as _, Background, Bounds, TestAppContext,
    component::{ActiveTheme as _, Root},
    point, px, size,
};

use super::{draft::RequestSection, tests::new_draft};

#[gpui_kit::test]
fn body_and_script_clicks_cancel_pending_vim_commands(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::update(cx, |p| p.vim_mode = true).unwrap();
        request_eagle_theme::init(cx);
    });

    for (section, phase) in [
        (RequestSection::Body, request::ScriptPhase::PreRequest),
        (RequestSection::Scripts, request::ScriptPhase::PreRequest),
        (RequestSection::Scripts, request::ScriptPhase::PostResponse),
    ] {
        let mut editor = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let draft = cx.new(|cx| {
                let mut draft = new_draft(cx);
                draft.request.method = collection::Method::Post;
                draft.section = section;
                draft
                    .script_editor(cx)
                    .update(cx, |scripts, _| scripts.phase = phase);
                draft.prepare(window, cx);
                let state = if section == RequestSection::Body {
                    draft.body_state(window, cx)
                } else {
                    draft.script_state(window, cx)
                };
                state.update(cx, |state, cx| {
                    state.replace_all("one two three", window, cx);
                    state.set_selected_range(0..0, cx);
                    state.focus(window, cx);
                });
                editor = Some(state);
                draft
            });
            Root::new(draft, window, cx)
        });
        let editor = editor.unwrap();

        for pending in ["d", "2", "g", "d g"] {
            cx.simulate_keystrokes("0");
            let point = cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                editor.read(cx).cursor_layout().unwrap().0.center()
            });
            cx.simulate_keystrokes(pending);
            cx.simulate_click(point, gpui_kit::Modifiers::default());
            cx.simulate_keystrokes("w");
            cx.read(|cx| {
                assert_eq!(editor.read(cx).value(), "one two three", "{pending}");
                assert_eq!(editor.read(cx).cursor(), 4, "{pending}");
            });
        }
    }
}

#[gpui_kit::test]
fn cached_request_editors_repaint_the_vim_block_after_cursor_motion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::update(cx, |p| p.vim_mode = true).unwrap();
        request_eagle_theme::init(cx);
    });

    for section in [RequestSection::Body, RequestSection::Scripts] {
        let mut editor = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let draft = cx.new(|cx| {
                let mut draft = new_draft(cx);
                draft.request.method = collection::Method::Post;
                draft.request.body =
                    Some(b"{\"first\": 1,\n\"second\": 2,\n\"third\": 3}".to_vec());
                draft.request.scripts.pre_request =
                    "let first = 1;\nlet second = 2;\nlet third = 3;".into();
                draft.section = section;
                draft.prepare(window, cx);

                let state = if section == RequestSection::Body {
                    draft.body_state(window, cx)
                } else {
                    draft.script_state(window, cx)
                };
                state.update(cx, |state, cx| {
                    state.set_selected_range(3..3, cx);
                    state.focus(window, cx);
                });
                editor = Some(state);
                draft
            });

            Root::new(draft, window, cx)
        });
        cx.simulate_resize(size(px(1440.), px(900.)));
        cx.run_until_parked();
        let editor = editor.unwrap();

        for keys in ["l", "j", "l", "k", "h"] {
            let previous = cx.read(|cx| editor.read(cx).cursor());
            cx.simulate_keystrokes(keys);

            // Inspect the frame produced by the actual keystroke. A forced
            // refresh here would hide stale paint in the cached configuration.
            cx.update(|window, cx| {
                let editor = editor.read(cx);
                assert_ne!(editor.cursor(), previous);
                let (caret, height) = editor.cursor_layout().unwrap();
                let top_left = point(
                    caret.left(),
                    caret.top() + editor.scroll_offset().y - (height - caret.size.height) / 2.,
                );
                let bounds = Bounds::from_corners(
                    window.pixel_snap_point(top_left),
                    window.pixel_snap_point(top_left + point(caret.size.width, height)),
                )
                .scale(window.scale_factor());
                let color = Background::from(cx.theme().foreground);
                assert!(window.painted_quads().iter().any(|quad| {
                    quad.bounds.origin == bounds.origin
                        && quad.bounds.size.height == bounds.size.height
                        && quad.bounds.size.width > bounds.size.width
                        && quad.background == color
                }));
            });
        }
    }
}
