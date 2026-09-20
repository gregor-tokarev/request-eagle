use std::time::Duration;

use gpui_kit::{Modifiers, TestAppContext};
use request::{Execution, ExecutionError, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::{ResponseContent, ResponseView};

fn response(body: &[u8], content_type: &str) -> ResponseContent {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", content_type.parse().unwrap());

    ResponseContent::new(Execution {
        elapsed: Duration::from_millis(239),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body: body.to_vec(),
        }),
    })
}

#[test]
fn response_formatting_preserves_raw_bytes_and_limits_display_text() {
    let json = response(b"{\"a\":1}", "application/json");
    assert_eq!(json.raw, "{\"a\":1}");
    assert_eq!(json.pretty.as_deref(), Some("{\n  \"a\": 1\n}"));
    assert_eq!(json.language, "json");
    assert_eq!(json.http().body, b"{\"a\":1}");
    assert_eq!(response(b"<html></html>", "text/html").language, "html");
    assert_eq!(response(b"1.2.3.4", "text/plain").language, "text");
    assert!(response(b"{broken", "application/json").pretty.is_none());

    let oversized = "a".repeat(1_048_575) + "é";
    let large = response(oversized.as_bytes(), "text/plain");
    assert!(large.truncated);
    assert_eq!(large.raw.len(), 1_048_575);
    assert_eq!(large.http().body.len(), 1_048_577);
}

#[gpui_kit::test]
fn response_editor_is_readonly_and_errors_replace_previous_results(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = ResponseView::new();
        view.finish(Ok(response(b"{\"a\":1}", "application/json")), window, cx);
        view
    });
    let body = cx.debug_bounds("response-body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.simulate_input("overwrite");
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).editor.as_ref().unwrap().read(cx).value(),
            "{\n  \"a\": 1\n}"
        )
    });

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.start(cx);
            view.finish(
                Err(ExecutionError::Timeout {
                    timeout: Duration::from_millis(50),
                }),
                window,
                cx,
            );
        })
    });
    assert!(cx.debug_bounds("response-status").is_none());
    assert!(cx.debug_bounds("response-body").is_none());
    cx.read(|cx| assert_eq!(view.read(cx).message, "request timed out after 50ms"));
}
