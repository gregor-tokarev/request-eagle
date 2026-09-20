use std::time::Duration;

use gpui_kit::{AppContext as _, Modifiers, MouseButton, TestAppContext, point, px};
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
        let mut view = ResponseView::new(cx);
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

#[gpui_kit::test]
fn response_headers_and_cookies_support_selection_and_copy(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut content = response(b"ok", "text/plain");
    let Response::Http(http) = &mut content.execution.response;
    http.headers.append(
        "set-cookie",
        "session=abc==; Path=/; HttpOnly".parse().unwrap(),
    );
    http.headers
        .append("set-cookie", "locale=en; SameSite=Lax".parse().unwrap());
    http.headers
        .insert("x-literal", "<value> & **raw**".parse().unwrap());
    let header_rows: Vec<_> = http
        .headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_owned()))
        .collect();
    let cookie_rows = vec![
        ("session".to_owned(), "abc==; Path=/; HttpOnly".to_owned()),
        ("locale".to_owned(), "en; SameSite=Lax".to_owned()),
    ];
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            view
        });
        gpui_kit::component::Root::new(response, window, cx)
    });

    for (section, rows) in [("Headers", header_rows), ("Cookies", cookie_rows)] {
        let tab = cx
            .debug_bounds(format!("response-section-{section}").leak())
            .unwrap();
        cx.simulate_click(tab.center(), Modifiers::default());

        for (index, (name, value)) in rows.iter().enumerate() {
            for (column, expected) in [("name", name), ("value", value)] {
                let cell = cx
                    .debug_bounds(format!("response-header-{column}-{index}").leak())
                    .unwrap();
                let start = point(cell.left() + px(8.), cell.center().y);
                let end = point(cell.right() - px(8.), cell.center().y);
                cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
                cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
                cx.simulate_keystrokes("secondary-c");
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some(expected.as_str()),
                    "{section} {column} {index}"
                );
            }
        }

        // Selection can span columns and multiple rows, including duplicate headers.
        let first = cx.debug_bounds("response-header-name-0").unwrap();
        let last = cx
            .debug_bounds(format!("response-header-value-{}", rows.len() - 1).leak())
            .unwrap();
        let start = point(first.left() + px(8.), first.center().y);
        let end = point(last.right() - px(8.), last.bottom() - px(1.));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.simulate_keystrokes("secondary-c");
        let expected = rows
            .iter()
            .flat_map(|(name, value)| [name.as_str(), value.as_str()])
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(expected.as_str())
        );
    }
}
