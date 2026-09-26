use gpui_kit::{Modifiers, TestAppContext};
use request::ScriptPhase;

use super::{
    draft::RequestSection,
    tests::{draft, element_bounds},
};

#[gpui_kit::test]
fn script_editors_keep_independent_drafts_and_snippets(cx: &mut TestAppContext) {
    let (draft, cx) = draft(cx);
    let scripts = element_bounds(cx, "request-section-Scripts").unwrap();
    cx.simulate_click(scripts.center(), Modifiers::default());
    assert!(element_bounds(cx, "request-scripts").is_some());

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            let editor = draft.script_state(window, cx);
            editor.update(cx, |editor, cx| {
                editor.replace_all("pm.variables.set('name', 'eagle');", window, cx)
            });
        });
    });
    let post = element_bounds(cx, "script-phase-Post-response").unwrap();
    cx.simulate_click(post.center(), Modifiers::default());
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            let editor = draft.script_state(window, cx);
            assert_eq!(editor.read(cx).language_name(), "javascript");
            assert!(editor.read(cx).value().is_empty());
            editor.update(cx, |editor, cx| {
                editor.replace_all(
                    "pm.test('ok', () => pm.response.to.have.status(200));",
                    window,
                    cx,
                )
            });
        });
    });
    let pre = element_bounds(cx, "script-phase-Pre-request").unwrap();
    cx.simulate_click(pre.center(), Modifiers::default());
    cx.read(|cx| {
        let draft = draft.read(cx);
        assert!(draft.is_dirty());
        assert_eq!(draft.script_phase, ScriptPhase::PreRequest);
        assert_eq!(
            draft.request.scripts.pre_request,
            "pm.variables.set('name', 'eagle');"
        );
        assert!(draft.request.scripts.post_response.starts_with("pm.test"));
        assert_eq!(
            draft.script_editors[0].as_ref().unwrap().read(cx).value(),
            draft.request.scripts.pre_request
        );
    });

    // Switching away does not recreate the editors or discard their text.
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.section = RequestSection::Headers;
            draft.prepare(window, cx);
            draft.section = RequestSection::Scripts;
            draft.prepare(window, cx);
            draft.mark_saved(draft.request.clone(), cx);
            assert!(!draft.is_dirty());
        });
    });
    assert!(element_bounds(cx, "script-snippets").is_some());
}

#[gpui_kit::test]
async fn pre_request_error_is_visible_without_an_http_response(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (draft, cx) = draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = "http://127.0.0.1:1".into();
            draft.request.scripts.pre_request =
                "console.log('before failure'); throw new Error('Fix the token');".into();
            draft.send(window, cx);
        });
    });
    let started = std::time::Instant::now();
    while cx.read(|cx| draft.read(cx).is_sending()) {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        smol::Timer::after(std::time::Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    assert!(element_bounds(cx, "script-test-results").is_some());
    let console = element_bounds(cx, "response-section-Console").unwrap();
    cx.simulate_click(console.center(), Modifiers::default());
    assert!(element_bounds(cx, "script-console").is_some());
}

#[gpui_kit::test]
fn scripts_fit_zoom_themes_and_resizing(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};
    let (draft, cx) = draft(cx);
    for theme in ["Default Light", "Default Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|window, cx| {
                assert!(request_eagle_theme::apply(theme, cx));
                gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size);
                window.set_rem_size(px(font_size));
                draft.update(cx, |draft, cx| {
                    draft.section = RequestSection::Scripts;
                    draft.prepare(window, cx);
                });
                window.refresh();
            });
            cx.simulate_resize(size(px(40. * font_size), px(40. * font_size)));
            let scripts = element_bounds(cx, "request-scripts").unwrap();
            let editor = element_bounds(cx, "script-editor").unwrap();
            let snippets = element_bounds(cx, "script-snippets").unwrap();
            assert!(
                editor.size.width >= px(16. * font_size),
                "editor: {editor:?}"
            );
            assert!(
                editor.size.height >= px(4. * font_size),
                "editor: {editor:?}"
            );
            assert!(snippets.right() <= scripts.right());
            assert!(snippets.bottom() <= scripts.bottom());
        }
    }
}
