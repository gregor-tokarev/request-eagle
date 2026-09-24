use std::time::Duration;

use gpui_kit::{Modifiers, TestAppContext};
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
