use std::time::Duration;

use collection::{HttpRequest, Method};
use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
use smol::io::{AsyncReadExt, AsyncWriteExt};

use super::{RequestDraft, draft::RequestSection, execution::outgoing_request};

// Debug selectors are collected only during layout, not replayed from cached
// controls. Refresh before querying geometry; interactions still use real input.
pub(super) fn element_bounds(
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    cx.update(|window, _| window.refresh());
    cx.debug_bounds(selector)
}

pub(super) fn draft(cx: &mut TestAppContext) -> (Entity<RequestDraft>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    cx.add_window_view(|window, cx| {
        let mut draft = RequestDraft::new();
        draft.prepare(window, cx);
        draft
    })
}

#[test]
fn prepares_json_requests_without_mutating_the_draft() {
    let mut original = HttpRequest {
        method: Method::Post,
        path: "  ifconfig.me/ip  ".into(),
        body: Some(b"{\"hello\":true}".to_vec()),
        ..HttpRequest::default()
    };
    let templated = HttpRequest {
        path: "{{baseUrl}}/echo".into(),
        ..Default::default()
    };
    assert_eq!(outgoing_request(&templated).path, "{{baseUrl}}/echo");
    let outgoing = outgoing_request(&original);
    assert_eq!(outgoing.path, "https://ifconfig.me/ip");
    assert_eq!(
        outgoing.headers,
        [("Content-Type".into(), "application/json".into())]
    );
    assert!(original.headers.is_empty());
    assert_eq!(original.path, "  ifconfig.me/ip  ");

    original
        .headers
        .push(("content-type".into(), "application/custom+json".into()));
    assert_eq!(outgoing_request(&original).headers, original.headers);

    for method in [Method::Get, Method::Head] {
        original.method = method;
        assert!(outgoing_request(&original).body.is_none());
        assert!(original.body.is_some());
    }
}

#[gpui_kit::test]
fn json_editor_is_only_available_for_body_methods_and_preserves_text(cx: &mut TestAppContext) {
    let (draft, cx) = draft(cx);
    let body_tab = element_bounds(cx, "request-section-Body").unwrap();
    cx.simulate_click(body_tab.center(), Modifiers::default());
    assert!(element_bounds(cx, "request-body").is_none());

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Post, cx);
            draft.prepare(window, cx);
        })
    });
    let body_tab = element_bounds(cx, "request-section-Body").unwrap();
    cx.simulate_click(body_tab.center(), Modifiers::default());
    let editor = cx.read(|cx| draft.read(cx).body.as_ref().unwrap().clone());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.replace_all("{\"hello\":true}", window, cx)
        })
    });
    cx.read(|cx| {
        assert_eq!(editor.read(cx).language_name(), "json");
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"{\"hello\":true}".as_slice())
        );
    });

    let format = element_bounds(cx, "format-request-json").unwrap();
    cx.simulate_click(format.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"{\n  \"hello\": true\n}".as_slice())
        )
    });

    for method in [Method::Get, Method::Head] {
        cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                draft.set_method(method, cx);
                draft.prepare(window, cx);
            })
        });
        assert!(element_bounds(cx, "request-body").is_none());
        cx.read(|cx| assert!(draft.read(cx).request.body.is_some()));
    }

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Put, cx);
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
        })
    });
    assert!(element_bounds(cx, "request-body").is_some());
    cx.read(|cx| assert_eq!(editor.read(cx).value(), "{\n  \"hello\": true\n}"));
}

#[gpui_kit::test]
async fn send_button_performs_a_request_and_displays_the_response(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/test", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let mut body = [0; 14];
        stream.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"{\"hello\":true}");
        assert!(
            String::from_utf8(head)
                .unwrap()
                .contains("content-type: application/json")
        );
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 15\r\nSet-Cookie: session=test; HttpOnly\r\nConnection: close\r\n\r\n{\"found\":false}").await.unwrap();
    });
    let (draft, cx) = draft(cx);
    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = url;
            draft.request.body = Some(b"{\"hello\":true}".to_vec());
            draft.set_method(Method::Post, cx);
        })
    });
    let send = element_bounds(cx, "send-request").unwrap();
    cx.simulate_click(send.center(), Modifiers::default());
    let started = std::time::Instant::now();
    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "request did not finish"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    server.await;
    assert!(element_bounds(cx, "response-status").is_some());
    assert!(element_bounds(cx, "response-body").is_some());

    let copy = element_bounds(cx, "response-copy").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().unwrap(),
        "{\"found\":false}"
    );
    let headers = element_bounds(cx, "response-section-Headers").unwrap();
    cx.simulate_click(headers.center(), Modifiers::default());
    assert!(element_bounds(cx, "response-header-table").is_some());
    let cookies = element_bounds(cx, "response-section-Cookies").unwrap();
    cx.simulate_click(cookies.center(), Modifiers::default());
    assert!(element_bounds(cx, "response-header-table").is_some());
}

#[gpui_kit::test]
fn cancel_button_releases_the_task_and_allows_sending_again(cx: &mut TestAppContext) {
    let (draft, cx) = draft(cx);
    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.task = Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
            cx.notify();
        })
    });
    let cancel = element_bounds(cx, "send-request").unwrap();
    cx.simulate_click(cancel.center(), Modifiers::default());
    cx.read(|cx| assert!(draft.read(cx).task.is_none()));
    assert!(element_bounds(cx, "response-empty").is_some());
}
