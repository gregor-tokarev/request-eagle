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
fn response_formatting_preserves_the_entire_body() {
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
    assert_eq!(large.raw.as_ref(), oversized);
    assert_eq!(large.raw.len(), 1_048_577);
    assert_eq!(large.http().body.len(), 1_048_577);
}

#[gpui_kit::test]
fn large_response_stays_raw_and_retains_search_copy_and_wrapping(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let raw = format!(
        "[{}\"needle ☃ tail\"]",
        "{\"name\":\"memory test\"},".repeat(60_000)
    );
    let content = response(raw.as_bytes(), "application/json");
    assert!(content.raw_only);
    assert!(content.pretty.is_none());
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            view
        });
        view = Some(response.clone());
        gpui_kit::component::Root::new(response, window, cx)
    });
    let view = view.unwrap();
    assert!(cx.debug_bounds("response-raw-only").is_some());
    let format = cx.debug_bounds("response-format").unwrap();
    cx.simulate_click(format.center(), Modifiers::default());
    cx.run_until_parked();
    cx.read(|cx| assert!(!view.read(cx).pretty));

    for (is_pretty, expected) in [
        (false, raw.as_str()),
        (true, raw.as_str()),
        (false, raw.as_str()),
    ] {
        cx.update(|window, cx| view.update(cx, |view, cx| view.set_pretty(is_pretty, window, cx)));
        cx.update(|window, cx| view.update(cx, |view, cx| view.open_response_search(window, cx)));
        cx.simulate_input("needle ☃ tail");
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(!view.pretty);
            assert!(view.editor.is_none());
            assert!(view.wrap);
            let body = view.virtual_body.as_ref().unwrap().read(cx);
            assert!(body.wrap);
            assert_eq!(body.text.to_string(), expected);
            let start = expected.find("needle ☃ tail").unwrap();
            assert!(start > 1024 * 1024, "search must reach past the old cutoff");
            assert_eq!(body.selection, start..start + "needle ☃ tail".len());
            assert!(
                body.painted.iter().any(|row| row.range.contains(&start)),
                "search must reveal the match on screen"
            );
        });
        let copy = cx.debug_bounds("response-copy").unwrap();
        cx.simulate_click(copy.center(), Modifiers::default());
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(raw.as_str())
        );
    }
    let body = cx.debug_bounds("response-body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some(raw.as_str())
    );

    for _ in 0..2 {
        let wrap = cx.debug_bounds("response-wrap").unwrap();
        cx.simulate_click(wrap.center(), Modifiers::default());
    }
    cx.read(|cx| assert!(view.read(cx).virtual_body.as_ref().unwrap().read(cx).wrap));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.finish(Ok(response(b"short response", "text/plain")), window, cx)
        })
    });
    cx.read(|cx| {
        assert!(view.read(cx).virtual_body.is_some());
        assert!(view.read(cx).wrap);
    });
}

#[gpui_kit::test]
fn response_editor_is_readonly_and_errors_replace_previous_results(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
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
        preferences::init(cx);
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
        preferences::init(cx);
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
                encoded_response_body_bytes: None,
            };
            http.headers.append(
                "set-cookie",
                "underlying-cookie=do-not-copy; Path=/".parse().unwrap(),
            );
            view.finish(Ok(ResponseContent::new(content.execution)), window, cx);
            view
        });
        gpui_kit::component::Root::new(response, window, cx)
    });
    let cookies = cx.debug_bounds("response-section-Cookies").unwrap();
    cx.simulate_click(cookies.center(), Modifiers::default());

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
            "Response size\n45 B\nHeaders (estimated)\n42 B\nDownloaded body\n3 B\nUncompressed\n3 B\nRequest size (known)\n16 B\nPrepared headers\n10 B\nBody\n6 B",
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
        assert!(
            !copied.contains("underlying-cookie"),
            "overlay selection leaked: {copied}"
        );
    }

    cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    let cell = cx.debug_bounds("response-header-value-0").unwrap();
    let start = point(cell.left() + px(8.), cell.center().y);
    let end = point(cell.right() - px(8.), cell.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("secondary-c");
    assert!(
        cx.read_from_clipboard()
            .unwrap()
            .text()
            .unwrap()
            .contains("do-not-copy")
    );
}

#[gpui_kit::test]
fn decoded_response_sizes_show_downloaded_and_uncompressed_bytes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            let mut content = response(&[b'a'; 1_024], "text/plain");
            let Response::Http(http) = &mut content.execution.response;
            http.headers
                .insert("content-encoding", "gzip".parse().unwrap());
            http.metrics.encoded_response_body_bytes = Some(29);
            http.metrics.response_header_bytes = 42;
            view.finish(Ok(content), window, cx);
            view
        });
        gpui_kit::component::Root::new(response, window, cx)
    });
    let trigger = cx.debug_bounds("response-size").unwrap();
    cx.simulate_mouse_move(trigger.center(), None, Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();

    let first = cx.debug_bounds("detail-response-total").unwrap();
    let last = cx.debug_bounds("detail-response-decoded").unwrap();
    let start = point(first.left() + px(1.), first.center().y);
    let end = point(last.right() - px(1.), last.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("secondary-c");

    let copied = cx.read_from_clipboard().unwrap().text().unwrap();
    assert!(
        copied.contains("Response size\n71 B\nHeaders (estimated)\n42 B\nDownloaded body\n29 B\nUncompressed\n1.0 KB (1024 B)"),
        "{copied}"
    );
}

#[gpui_kit::test]
fn timing_waterfall_uses_one_elapsed_time_axis(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
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
        preferences::init(cx);
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

#[test]
fn response_size_limits_lock_raw_for_long_lines_and_pretty_expansion() {
    let cases = [
        format!("\"{}\"", "a".repeat(32 * 1024)),
        format!("[\n{}0]", "0,\n".repeat(70_000)),
        format!("[\n{}0]", "0,\n".repeat(90_000)),
    ];
    for raw in cases {
        let content = response(raw.as_bytes(), "application/json");
        assert!(content.raw_only);
        assert!(content.pretty.is_none());
        assert_eq!(content.raw.as_ref(), raw);
    }
    assert!(!response(&vec![b'a'; 32 * 1024], "text/plain").raw_only);
    let at_limit = "1234567\n".repeat(32 * 1024);
    assert!(!response(at_limit.as_bytes(), "text/plain").raw_only);
}

#[gpui_kit::test]
fn raw_uses_plain_viewer_and_json_restores_highlighted_editor(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let raw = "{\"a\":1,\"message\":\"needle\"}";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = ResponseView::new(cx);
        view.finish(Ok(response(raw.as_bytes(), "application/json")), window, cx);
        view
    });
    for pretty in [false, true, false] {
        cx.update(|window, cx| view.update(cx, |view, cx| view.set_pretty(pretty, window, cx)));
        cx.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.pretty, pretty);
            assert_eq!(view.editor.is_some(), pretty);
            assert_eq!(view.virtual_body.is_some(), !pretty);
            if let Some(body) = &view.virtual_body {
                assert_eq!(body.read(cx).text.to_string(), raw);
            }
        });
    }
    let bounds = cx.debug_bounds("response-body").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some(raw)
    );
    cx.simulate_input("overwrite");
    cx.read(|cx| {
        assert_eq!(
            view.read(cx)
                .virtual_body
                .as_ref()
                .unwrap()
                .read(cx)
                .text
                .to_string(),
            raw
        )
    });
}
