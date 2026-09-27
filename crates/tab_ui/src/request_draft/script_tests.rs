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

fn answer_trust_prompt(cx: &mut VisualTestContext, answer: &str) {
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    assert!(element_bounds(cx, "dialog-0").is_some());
    let controls = cx.update(|window, _| {
        gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|element| element.visible() && element.role() == Some(gpui_kit::Role::Button))
            .collect::<Vec<_>>()
    });
    let button = |label| {
        controls
            .iter()
            .find(|element| element.label() == Some(label))
            .unwrap_or_else(|| panic!("missing trust dialog button {label}"))
    };
    assert!(button("Trust and Send").bounds().size.width > gpui_kit::px(0.));
    assert!(button("Cancel").bounds().size.width > gpui_kit::px(0.));
    cx.simulate_click(button(answer).bounds().center(), Modifiers::default());
    cx.run_until_parked();
}

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
    let (draft, cx) = script_draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = "http://127.0.0.1:1".into();
            draft.request.scripts.pre_request =
                "console.log('before failure'); throw new Error('Fix the token');".into();
            draft.send(window, cx);
        });
    });
    answer_trust_prompt(cx, "Trust and Send");
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
async fn script_trust_gates_sends_and_is_bound_to_the_tab_and_exact_scripts(
    cx: &mut TestAppContext,
) {
    use smol::io::{AsyncReadExt, AsyncWriteExt};
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = smol::channel::bounded(2);
    let server = smol::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }
            assert!(
                String::from_utf8(head)
                    .unwrap()
                    .contains("authorization: Bearer secret\r\n")
            );
            let mut body = [0; 6];
            stream.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"secret");
            sent.send(()).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let (draft, cx) = script_draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            *draft = RequestDraft::from_saved(
                "Imported".into(),
                "API".into(),
                request::HttpRequest {
                    method: request::Method::Post,
                    path: "https://original.example".into(),
                    headers: vec![("Authorization".into(), "Bearer secret".into())],
                    body: Some(b"secret".to_vec()),
                    scripts: request::RequestScripts {
                        pre_request: format!("pm.request.url = {destination:?};"),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            draft.send(window, cx);
            draft.send(window, cx);
            assert!(!draft.is_sending());
            assert!(draft.response.is_none());
        });
    });
    assert!(
        element_bounds(cx, "dialog-1").is_none(),
        "repeated Send must not stack prompts"
    );
    answer_trust_prompt(cx, "Cancel");
    assert!(received.try_recv().is_err());
    assert!(element_bounds(cx, "request-scripts").is_some());

    // An approval for old source must not authorize a concurrent replacement.
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.send(window, cx);
            draft
                .request
                .scripts
                .pre_request
                .push_str(" console.log('changed');");
        });
    });
    answer_trust_prompt(cx, "Trust and Send");
    cx.read(|cx| {
        assert!(!draft.read(cx).is_sending());
        assert!(draft.read(cx).trusted_scripts.is_none());
    });
    assert!(received.try_recv().is_err());

    for run in 0..2 {
        cx.update(|window, cx| draft.update(cx, |draft, cx| draft.send(window, cx)));
        if run == 0 {
            answer_trust_prompt(cx, "Trust and Send");
        } else {
            assert!(element_bounds(cx, "dialog-0").is_none());
        }
        let started = Instant::now();
        while cx.read(|cx| draft.read(cx).is_sending()) {
            assert!(started.elapsed() < Duration::from_secs(5));
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }
        received.try_recv().expect("approved script should send");
    }
    server.await;

    // Either phase changing, or reopening the request, requires fresh approval.
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.scripts.post_response =
                "pm.test('status', () => pm.response.to.have.status(200));".into();
            draft.send(window, cx);
            assert!(!draft.is_sending());
        });
    });
    answer_trust_prompt(cx, "Cancel");
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            *draft = RequestDraft::from_saved(
                "Reopened".into(),
                "API".into(),
                draft.saved_request.clone(),
            );
            draft.send(window, cx);
            assert!(!draft.is_sending());
            assert!(draft.trusted_scripts.is_none());
        });
    });
    answer_trust_prompt(cx, "Cancel");
}

#[gpui_kit::test]
fn scripts_fit_zoom_themes_and_resizing(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};
    let (draft, cx) = script_draft(cx);
    cx.update(|_, cx| {
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

            cx.update(|window, cx| draft.update(cx, |draft, cx| draft.send(window, cx)));
            for _ in 0..2 {
                cx.update(|window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                });
            }
            let dialog = element_bounds(cx, "dialog-0").unwrap();
            assert!(dialog.origin.x >= px(0.) && dialog.origin.y >= px(0.));
            assert!(dialog.right() <= px(40. * font_size));
            assert!(dialog.bottom() <= px(40. * font_size));
            answer_trust_prompt(cx, "Cancel");
        }
    }
}
