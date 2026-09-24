use std::time::Duration;

use gpui_kit::{AppContext as _, Modifiers, TestAppContext, point, px};
use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use tab_ui::test_support::ResponseContent;

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

#[gpui_kit::test]
fn search_shortcut_preserves_response_search(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let (layout, cx) = cx.add_window_view(|window, cx| {
        crate::workspace::Layout::new(
            collection::CollectionRegistry::new(),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    });
    let view = cx.read(|cx| {
        layout.read(cx).main_view.read(cx).tabs[0]
            .page
            .view()
            .downcast::<tab_ui::RequestDraft>()
            .ok()
            .unwrap()
            .read(cx)
            .response_for_test()
    });

    // Neither the response's own focus nor a nested search input should
    // activate the workspace's sidebar search binding.
    for body in ["needle".to_owned(), "needle\n".repeat(200_000)] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.finish(Ok(response(body.as_bytes(), "text/plain")), window, cx);
                window.focus(&view.focus_for_test(), cx);
            });
        });
        cx.simulate_keystrokes("secondary-f");
        cx.update(|window, cx| {
            assert!(view.read(cx).focus_for_test().contains_focused(window, cx))
        });

        let bounds = cx.debug_bounds("response-body").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.simulate_keystrokes("secondary-f");
        cx.simulate_input("needle");
        cx.simulate_keystrokes("secondary-f");
        cx.update(|window, cx| {
            assert!(view.read(cx).focus_for_test().contains_focused(window, cx))
        });
        cx.read(|cx| {
            let view = view.read(cx);

            assert_eq!(view.search_query_for_test(cx), "needle");
        });
    }
}

#[gpui_kit::test]
fn minimum_workspace_keeps_request_fields_and_response_visible_at_each_zoom(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{InputEvent as _, ScrollDelta, ScrollWheelEvent, TouchPhase};

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    let mut layout = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            crate::workspace::Layout::new(
                collection::CollectionRegistry::new(),
                updater::init("1.2.3", cx),
                window,
                cx,
            )
        });
        layout = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let response_view = cx.read(|cx| {
        layout.as_ref().unwrap().read(cx).main_view.read(cx).tabs[0]
            .page
            .view()
            .downcast::<tab_ui::RequestDraft>()
            .ok()
            .unwrap()
            .read(cx)
            .response_for_test()
    });
    cx.update(|window, cx| {
        response_view.update(cx, |view, cx| {
            view.finish(
                Ok(response(b"A readable response", "text/plain")),
                window,
                cx,
            );
        });
    });

    for font_size in [12., 16., 24.] {
        cx.update(|window, cx| {
            gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size);
            window.refresh();
        });
        cx.simulate_resize(gpui_kit::size(px(40. * font_size), px(40. * font_size)));
        cx.run_until_parked();

        // Generated headers may overflow the request pane at its minimum.
        // Scrolling that pane must expose its editable row without moving the response.
        let request = cx.debug_bounds("request-section-content").unwrap();
        cx.update(|window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position: request.center(),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-500.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.run_until_parked();

        let body = cx.debug_bounds("response-body").unwrap();
        let field = cx.debug_bounds("headers-key-0").unwrap();
        let response_tab = cx.debug_bounds("response-section-Body").unwrap();
        assert!(
            body.size.height >= px(3. * font_size),
            "response body at {font_size} px: {body:?}"
        );
        assert!(
            field.bottom() <= response_tab.top(),
            "editable header should be above the response at {font_size} px: {field:?}, {response_tab:?}"
        );
        assert!(
            body.bottom() <= px(38. * font_size),
            "response must clear the status bar"
        );
    }
}
