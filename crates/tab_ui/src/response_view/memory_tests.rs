use std::time::Duration;

use gpui_kit::{
    AppContext as _, InputEvent as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, component::Root, point, px, size,
};
use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::{ResponseContent, ResponseView};

#[gpui_kit::test]
fn full_single_line_response_scroll_has_bounded_allocations(cx: &mut TestAppContext) {
    check_scroll_allocations(
        cx,
        "{\"name\":\"memory test\",\"active\":true},".repeat(300_000),
    );
}

#[gpui_kit::test]
fn full_multiline_response_scroll_has_bounded_allocations(cx: &mut TestAppContext) {
    check_scroll_allocations(
        cx,
        format!("{}\n", "long response row ".repeat(400)).repeat(1_500),
    );
}

fn check_scroll_allocations(cx: &mut TestAppContext, body: String) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let expected = body.len();
    assert!(expected > 10 * 1024 * 1024);
    let content = ResponseContent::new(Execution {
        scripts: Vec::new(),
        elapsed: Duration::from_millis(1),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers: HeaderMap::new(),
            body: body.into_bytes(),
            metrics: Default::default(),
        }),
    });
    assert_eq!(content.raw.len(), expected);
    let mut response_view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            view
        });
        response_view = Some(response.clone());
        Root::new(response, window, cx)
    });
    let response = response_view.unwrap();
    let viewer = cx.read(|cx| response.read(cx).virtual_body.clone().unwrap());
    cx.read(|cx| {
        assert!(viewer.read(cx).wrap);
        assert_eq!(viewer.read(cx).text.len(), expected);
    });

    for width in [1024., 640.] {
        cx.simulate_resize(size(px(width), px(768.)));
        cx.run_until_parked();
        let body = cx.debug_bounds("response-body").unwrap();
        let mut maximum = 0;
        for index in 0..12 {
            let before = cx.read(|cx| viewer.read(cx).scroll);
            let allocated = crate::test_allocator::allocated_by(|| {
                cx.update(|window, cx| {
                    window.dispatch_event(
                        ScrollWheelEvent {
                            position: body.center(),
                            delta: ScrollDelta::Pixels(point(
                                px(0.),
                                px(if index < 6 { -100. } else { 100. }),
                            )),
                            modifiers: Modifiers::default(),
                            touch_phase: TouchPhase::Moved,
                        }
                        .to_platform_input(),
                        cx,
                    )
                });
                cx.run_until_parked();
            });
            if index == 0 {
                assert_ne!(cx.read(|cx| viewer.read(cx).scroll), before);
            }
            maximum = maximum.max(allocated);
        }
        eprintln!(
            "Full {expected} byte response, wrapped, width={width}px: maximum scroll allocation {maximum} bytes"
        );
        assert!(
            maximum < 4 * 1024 * 1024,
            "scrolling allocated {maximum} bytes"
        );
        cx.read(|cx| {
            let body = viewer.read(cx);
            assert!(body.painted.len() < 50);
            assert!(body.painted.iter().all(|row| row.line.len() < 1024));
        });
    }

    let body = cx.debug_bounds("response-body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-f");
    assert!(cx.debug_bounds("response-body-search").is_some());
    cx.simulate_input("e");
    cx.read(|cx| {
        assert!(
            response
                .read(cx)
                .body_search
                .as_ref()
                .unwrap()
                .matches
                .len()
                > 100_000
        )
    });
    let first = cx.read(|cx| viewer.read(cx).selection.clone());
    cx.simulate_keystrokes("enter");
    assert_ne!(cx.read(|cx| viewer.read(cx).selection.clone()), first);
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(cx.read(|cx| viewer.read(cx).selection.clone()), first);
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("secondary-end");
    // Exercise the action directly as macOS binds document end to cmd-down.
    cx.update(|window, cx| window.dispatch_action(Box::new(gpui_kit::base::input::MoveToEnd), cx));
    cx.read(|cx| {
        let body = viewer.read(cx);
        assert_eq!(body.selection.end, expected);
        assert_eq!(body.painted.last().unwrap().range.end, expected);
    });
}

#[gpui_kit::test]
fn moderate_raw_html_uses_plain_viewer_with_bounded_scroll_allocations(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    // About 90 KB, with individual HTML lines between 16 and 32 KiB.
    let line = format!(
        "{}\n",
        "<p class=\"summary\">HTML response text for selection and searching.</p>".repeat(340)
    );
    assert!(line.len() > 16 * 1024 && line.len() < 32 * 1024);
    let body = format!(
        "<!doctype html><html><body>\n{}</body></html>",
        line.repeat(4)
    );
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/html".parse().unwrap());
    let content = ResponseContent::new(Execution {
        scripts: Vec::new(),
        elapsed: Duration::from_millis(1),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body: body.as_bytes().to_vec(),
            metrics: Default::default(),
        }),
    });
    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            assert!(view.editor.is_none());
            assert!(view.wrap);
            editor = view.virtual_body.clone();
            view
        });
        Root::new(response, window, cx)
    });
    let editor = editor.expect("Raw HTML should use the plain viewer");
    for width in [1024., 640.] {
        cx.simulate_resize(size(px(width), px(768.)));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("response-body").unwrap();
        let before = cx.read(|cx| editor.read(cx).scroll);
        let mut maximum = 0;
        for _ in 0..8 {
            let allocated = crate::test_allocator::allocated_by(|| {
                cx.update(|window, cx| {
                    window.dispatch_event(
                        ScrollWheelEvent {
                            position: bounds.center(),
                            delta: ScrollDelta::Pixels(point(px(0.), px(-50.))),
                            modifiers: Modifiers::default(),
                            touch_phase: TouchPhase::Moved,
                        }
                        .to_platform_input(),
                        cx,
                    )
                });
                cx.run_until_parked();
            });
            maximum = maximum.max(allocated);
        }
        assert_ne!(cx.read(|cx| editor.read(cx).scroll), before);
        assert!(
            maximum < 16 * 1024 * 1024,
            "Raw HTML scroll allocated {maximum} bytes at {width}px"
        );
        eprintln!("Raw moderate HTML {width}px: maximum scroll allocations {maximum} bytes");
    }
    let bounds = cx.debug_bounds("response-body").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some(body.as_str())
    );
    cx.simulate_input("overwrite");
    cx.read(|cx| assert_eq!(editor.read(cx).text.to_string(), body));
}
