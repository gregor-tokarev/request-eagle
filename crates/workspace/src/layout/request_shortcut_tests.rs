use collection::Method;
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
use smol::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;
use tab_ui::RequestDraft;

fn element_bounds(
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    cx.update(|window, _| window.refresh());
    cx.debug_bounds(selector)
}

#[gpui_kit::test]
async fn send_shortcut_uses_the_active_request_from_inputs_and_response(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/shortcut", listener.local_addr().unwrap());
    let (sent, received) = smol::channel::bounded(4);
    let server = smol::spawn(async move {
        for _ in 0..4 {
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
                    .starts_with("POST /shortcut ")
            );
            let mut body = [0; 14];
            stream.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"{\"hello\":true}");
            sent.send(()).await.unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await.unwrap();
        }
    });

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
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    let first = cx.read(|cx| {
        tabs.read(cx).tabs[0]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.simulate_keystrokes("secondary-t");
    let active = cx.read(|cx| {
        tabs.read(cx).tabs[1]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.update(|window, _| window.refresh());
    let url_bounds = element_bounds(cx, "request-url").unwrap();
    cx.simulate_click(url_bounds.center(), Modifiers::default());
    cx.simulate_input(&url);
    cx.update(|_, cx| active.update(cx, |draft, cx| draft.set_method(Method::Post, cx)));
    let body_tab = element_bounds(cx, "request-section-Body").unwrap();
    cx.simulate_click(body_tab.center(), Modifiers::default());
    let body = element_bounds(cx, "request-body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.simulate_input("{\"hello\":true}");

    for selector in [
        "request-url",
        "request-body",
        "response-body",
        "script-editor",
    ] {
        if selector == "script-editor" {
            let tab = element_bounds(cx, "request-section-Scripts").unwrap();
            cx.simulate_click(tab.center(), Modifiers::default());
        }
        let bounds = element_bounds(cx, selector).unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.simulate_keystrokes("secondary-enter");
        let started = std::time::Instant::now();
        while cx.read(|cx| active.read(cx).is_sending()) {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "shortcut request did not finish"
            );
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }
        assert!(
            element_bounds(cx, "response-status").is_some(),
            "shortcut from {selector}"
        );
        received
            .try_recv()
            .expect("shortcut must send a new request");
        cx.read(|cx| {
            assert_eq!(
                active.read(cx).request.body.as_deref(),
                Some(b"{\"hello\":true}".as_slice())
            );
            assert!(!first.read(cx).is_sending());
            assert!(first.read(cx).request.path.is_empty());
        });
    }
    server.await;

    // Repeating the shortcut must not cancel an in-flight request.
    cx.update(|_, cx| {
        active.update(cx, |draft, cx| {
            draft.hold_request_for_test(cx);
            cx.notify();
        })
    });
    cx.simulate_keystrokes("secondary-enter");
    cx.read(|cx| assert!(active.read(cx).is_sending()));
    cx.update(|_, cx| active.update(cx, |draft, cx| draft.cancel(cx)));
}
