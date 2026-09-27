use gpui_kit::{
    AppContext, Context, Entity, IntoElement, Modifiers, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, component::Root, div,
};
use request::ScriptPhase;

use super::{
    RequestDraft,
    draft::RequestSection,
    tests::{draft, element_bounds},
};

struct ScriptTestWindow(Entity<RequestDraft>);

impl Render for ScriptTestWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.0.clone())
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn script_draft(cx: &mut TestAppContext) -> (Entity<RequestDraft>, &mut VisualTestContext) {
    // Script assistance uses the shared TypeScript worker outside GPUI's test executor.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = RequestDraft::new();
            view.prepare(window, cx);
            view
        });
        draft = Some(view.clone());
        let host = cx.new(|_| ScriptTestWindow(view));
        Root::new(host, window, cx)
    });
    (draft.unwrap(), cx)
}

#[gpui_kit::test]
fn vim_scripts_accept_keyboard_input_after_clicking_the_editor(cx: &mut TestAppContext) {
    let (draft, cx) = script_draft(cx);
    cx.update(|_, cx| preferences::update(cx, |p| p.vim_mode = true).unwrap());
    let scripts = element_bounds(cx, "request-section-Scripts").unwrap();
    cx.simulate_click(scripts.center(), Modifiers::default());

    for phase in ["Pre-request", "Post-response"] {
        let phase_tab = element_bounds(
            cx,
            if phase == "Pre-request" {
                "script-phase-Pre-request"
            } else {
                "script-phase-Post-response"
            },
        )
        .unwrap();
        cx.simulate_click(phase_tab.center(), Modifiers::default());
        let bounds = element_bounds(cx, "script-editor").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.simulate_keystrokes("h j k l i a b c escape 0 l l l x");
        cx.read(|cx| {
            let scripts = &draft.read(cx).request.scripts;
            let value = if phase == "Pre-request" {
                &scripts.pre_request
            } else {
                &scripts.post_response
            };
            assert_eq!(value, "ab", "{phase}");
        });
    }
}

#[gpui_kit::test]
fn vim_edits_update_body_and_both_script_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (draft, cx) = draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(collection::Method::Post, cx);
            draft.section = RequestSection::Body;
            draft
                .body_state(window, cx)
                .update(cx, |body, cx| body.focus(window, cx));
            cx.notify();
        });
        preferences::update(cx, |p| p.vim_mode = true).unwrap();
    });
    cx.simulate_keystrokes("i");
    cx.simulate_input("{\"ok\":true}");
    cx.simulate_keystrokes("escape");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"{\"ok\":true}".as_slice())
        )
    });

    for phase in [ScriptPhase::PreRequest, ScriptPhase::PostResponse] {
        cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                draft.section = RequestSection::Scripts;
                draft.script_phase = phase;
                draft
                    .script_state(window, cx)
                    .update(cx, |editor, cx| editor.focus(window, cx));
                cx.notify();
            });
        });
        cx.simulate_keystrokes("i");
        cx.simulate_input("console.log('vim');");
        cx.simulate_keystrokes("escape 0 x");
    }

    cx.read(|cx| {
        let draft = draft.read(cx);
        assert!(draft.is_dirty());
        assert_eq!(draft.request.scripts.pre_request, "onsole.log('vim');");
        assert_eq!(draft.request.scripts.post_response, "onsole.log('vim');");
    });
}

#[gpui_kit::test]
fn script_editors_keep_independent_drafts_and_snippets(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
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
    let (draft, cx) = script_draft(cx);
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
async fn saved_and_edited_scripts_run_on_send_without_confirmation(cx: &mut TestAppContext) {
    use smol::io::{AsyncReadExt, AsyncWriteExt};
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = smol::channel::bounded(3);
    let server = smol::spawn(async move {
        for expected in ["initial", "edited", "initial"] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }
            let head = String::from_utf8(head).unwrap();
            assert!(
                head.contains(&format!("x-script: {expected}\r\n")),
                "{head}"
            );
            sent.send(()).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let (draft, cx) = script_draft(cx);
    let saved = request::HttpRequest {
        path: destination,
        scripts: request::RequestScripts {
            pre_request: "pm.request.headers.upsert({key: 'X-Script', value: 'initial'});".into(),
            post_response: "pm.test('status', () => pm.response.to.have.status(200));".into(),
        },
        ..Default::default()
    };

    for run in 0..3 {
        cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                if run == 1 {
                    draft.request.scripts.pre_request = draft
                        .request
                        .scripts
                        .pre_request
                        .replace("initial", "edited");
                    draft
                        .request
                        .scripts
                        .post_response
                        .push_str(" console.log('edited');");
                } else {
                    *draft = RequestDraft::from_saved("Saved".into(), "API".into(), saved.clone());
                }
                draft.prepare(window, cx);
                cx.notify();
            });
        });
        let send = element_bounds(cx, "send-request").unwrap();
        cx.simulate_click(send.center(), Modifiers::default());
        assert!(element_bounds(cx, "dialog-0").is_none());

        let started = Instant::now();
        while cx.read(|cx| draft.read(cx).is_sending()) {
            assert!(started.elapsed() < Duration::from_secs(5));
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }
        received
            .try_recv()
            .expect("Send should run scripts immediately");
        let tests = element_bounds(cx, "response-section-Test Results").unwrap();
        cx.simulate_click(tests.center(), Modifiers::default());
        assert!(element_bounds(cx, "script-test-results").is_some());
    }
    server.await;
}

#[gpui_kit::test]
fn scripts_fit_zoom_themes_and_resizing(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};
    let (draft, cx) = script_draft(cx);
    cx.update(|_, cx| {
        preferences::update(cx, |p| p.vim_mode = true).unwrap();
        draft.update(cx, |draft, _| {
            draft.request.scripts.pre_request = "console.log('review');".into();
        });
    });
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
            let mode = element_bounds(cx, "vim-mode-indicator").unwrap();
            assert!(mode.right() <= scripts.right());
            assert!(mode.bottom() <= scripts.bottom());
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
