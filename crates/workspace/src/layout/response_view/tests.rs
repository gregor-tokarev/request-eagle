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
            metrics: request::HttpMetrics::default(),
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
    let content = ResponseContent::new(content.execution);
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
        let end = point(last.left() + px(200.), last.center().y);
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

#[gpui_kit::test]
fn response_overlays_open_on_hover_and_copy_details_in_order(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            let mut content = response(b"abc", "text/plain");
            let Response::Http(http) = &mut content.execution.response;
            http.metrics = request::HttpMetrics {
                prepare: Duration::from_millis(1),
                waiting: Duration::from_millis(150),
                download: Duration::from_millis(80),
                response_header_bytes: 42,
                request_header_bytes: 10,
                request_body_bytes: 6,
            };
            view.finish(Ok(content), window, cx);
            view
        });
        gpui_kit::component::Root::new(response, window, cx)
    });

    for (trigger, panel, first, last, expected) in [
        (
            "response-time",
            "response-time-overlay",
            "detail-time-title",
            "timing-phase-2",
            "Prepare\n1.00 ms\nWait for headers\n150.00 ms\nDownload\n80.00 ms",
        ),
        (
            "response-size",
            "response-size-overlay",
            "detail-response-total",
            "detail-request-body",
            "Response size\n45 B\nHeaders (estimated)\n42 B\nDownloaded body\n3 B\nUncompressed\n3 B\nRequest size (known)\n16 B\nConfigured headers\n10 B\nBody\n6 B",
        ),
    ] {
        cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        let trigger = cx.debug_bounds(trigger).unwrap();
        cx.simulate_mouse_move(trigger.center(), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        assert!(cx.debug_bounds(panel).is_some());

        let first = cx.debug_bounds(first).unwrap();
        let last = cx.debug_bounds(last).unwrap();
        let start = point(first.left() + px(1.), first.center().y);
        let end = point(last.right() - px(1.), last.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.simulate_keystrokes("secondary-c");
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(copied.contains(expected), "{panel}: {copied}");
    }
}

#[gpui_kit::test]
fn timing_waterfall_uses_one_elapsed_time_axis(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut response_view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            let mut content = response(b"ok", "text/plain");
            content.execution.elapsed = Duration::from_millis(250);
            content.processing = Duration::from_millis(50);
            let Response::Http(http) = &mut content.execution.response;
            http.metrics = request::HttpMetrics {
                prepare: Duration::from_millis(20),
                waiting: Duration::from_millis(150),
                download: Duration::from_millis(80),
                ..Default::default()
            };
            view.finish(Ok(content), window, cx);
            view
        });
        response_view = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });

    for width in [1024., 640.] {
        cx.simulate_resize(gpui_kit::size(px(width), px(768.)));
        cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        let trigger = cx.debug_bounds("response-time").unwrap();
        cx.simulate_mouse_move(trigger.center(), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();

        let plot = cx.debug_bounds("timing-plot").unwrap();
        let mut previous = None;
        for (index, (start, duration)) in [(0., 20.), (20., 150.), (170., 80.), (250., 50.)]
            .into_iter()
            .enumerate()
        {
            let bar = cx
                .debug_bounds(format!("timing-bar-{index}").leak())
                .unwrap();
            assert!((bar.left() - (plot.left() + plot.size.width * (start / 300.))).abs() < px(1.));
            assert!((bar.size.width - plot.size.width * (duration / 300.)).abs() < px(1.));
            assert!(bar.left() >= plot.left() && bar.right() <= plot.right() + px(1.));
            if let Some((right, bottom)) = previous {
                assert!(
                    (bar.left() - right).abs() < px(1.),
                    "phase bars should join end to start"
                );
                assert_eq!(bar.top(), bottom, "phase rows should be contiguous");
            }
            previous = Some((bar.right(), bar.bottom()));
        }
    }

    // Empty measurements must not produce NaN geometry or imply a duration.
    cx.update(|window, cx| {
        response_view.as_ref().unwrap().update(cx, |view, cx| {
            let mut content = response(b"", "text/plain");
            content.execution.elapsed = Duration::ZERO;
            content.processing = Duration::ZERO;
            view.finish(Ok(content), window, cx);
        });
    });
    assert!(cx.debug_bounds("timing-plot").is_some());
    for index in 0..4 {
        assert!(
            cx.debug_bounds(format!("timing-bar-{index}").leak())
                .is_none()
        );
    }
}

#[gpui_kit::test]
fn virtual_headers_keep_scrolled_and_wrapped_values_selectable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut content = response(b"ok", "text/plain");
    let Response::Http(http) = &mut content.execution.response;
    for index in 0..128 {
        http.headers.insert(
            format!("x-header-{index}")
                .parse::<request::HeaderName>()
                .unwrap(),
            format!("value {index} {}end", "wrapped header text ".repeat(20))
                .parse()
                .unwrap(),
        );
    }
    let content = ResponseContent::new(content.execution);
    let last = content.headers.len() - 1;
    let expected = content.headers[last].1.clone();
    let mut response_view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            view
        });
        response_view = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let view = response_view.unwrap();
    let tab = cx.debug_bounds("response-section-Headers").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    let selector = format!("response-header-value-{last}").leak();
    assert!(
        cx.debug_bounds(selector).is_none(),
        "offscreen rows should not render"
    );

    for width in [1024., 640.] {
        cx.simulate_resize(gpui_kit::size(px(width), px(768.)));
        cx.update(|window, cx| {
            view.read(cx).headers_list.scroll_to(gpui_kit::ListOffset {
                item_ix: last,
                offset_in_item: px(0.),
            });
            window.refresh();
        });
        let cell = cx.debug_bounds(selector).unwrap();
        assert!(cell.size.height > px(30.), "long values must wrap");
        let start = point(cell.left() + px(8.), cell.top() + px(8.));
        let end = point(cell.right() - px(8.), cell.bottom() - px(8.));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            expected.as_ref()
        );
    }
}
