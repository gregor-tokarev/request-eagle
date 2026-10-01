use std::time::{Duration, Instant};

use gpui_kit::{App, Entity, Modifiers, TestAppContext, VisualTestContext};
use smol::io::{AsyncReadExt, AsyncWriteExt};

use super::RequestDraft;
use super::tests::{draft, element_bounds};

async fn wait_until(cx: &mut VisualTestContext, condition: impl Fn(&App) -> bool) {
    let started = Instant::now();

    while !cx.read(|cx| condition(cx)) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for the event stream"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
}

fn rows(draft: &Entity<RequestDraft>, cx: &App) -> Vec<(String, String)> {
    draft
        .read(cx)
        .response
        .read(cx)
        .events_for_test()
        .map_or_else(Vec::new, |events| events.read(cx).rows())
}

fn row(event: &str, preview: &str) -> (String, String) {
    (event.to_owned(), preview.to_owned())
}

#[gpui_kit::test]
async fn event_streams_show_their_events_until_stopped(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/events", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: hello\n\nevent: update\nid: 7\ndata: {\"n\":1}\n\n")
            .await
            .unwrap();

        // The stream stays open until the client stops it.
        let mut byte = [0];
        stream.read(&mut byte).await.unwrap()
    });
    let (draft, cx) = draft(cx);
    cx.update(|_, cx| draft.update(cx, |draft, _| draft.request.path = url));

    let send = element_bounds(cx, "send-request").unwrap();
    cx.simulate_click(send.center(), Modifiers::default());
    wait_until(cx, |cx| rows(&draft, cx).len() == 2).await;

    assert_eq!(
        cx.read(|cx| rows(&draft, cx)),
        [row("update", "{\"n\":1}"), row("message", "hello")]
    );
    cx.read(|cx| {
        let draft = draft.read(cx);
        assert!(draft.streaming && draft.task.is_some());
    });
    assert!(element_bounds(cx, "response-section-Events").is_some());
    assert!(element_bounds(cx, "response-streaming").is_some());
    assert!(element_bounds(cx, "response-metadata").is_none());

    let newest = element_bounds(cx, "response-event-0").unwrap();
    cx.simulate_click(newest.center(), Modifiers::default());
    assert!(element_bounds(cx, "response-event-detail-0").is_some());
    assert!(element_bounds(cx, "response-event-id-0").is_some());

    let stop = element_bounds(cx, "send-request").unwrap();
    cx.simulate_click(stop.center(), Modifiers::default());
    wait_until(cx, |cx| draft.read(cx).task.is_none()).await;

    assert_eq!(server.await, 0, "stopping must close the connection");
    cx.read(|cx| assert!(!draft.read(cx).streaming));
    assert_eq!(cx.read(|cx| rows(&draft, cx))[0], row("", "Stopped"));
    assert!(element_bounds(cx, "response-metadata").is_some());
    assert!(element_bounds(cx, "response-streaming").is_none());

    let search = element_bounds(cx, "response-events-search").unwrap();
    cx.simulate_click(search.center(), Modifiers::default());
    cx.simulate_input("HELLO");
    // How the stream ended stays in view.
    assert_eq!(
        cx.read(|cx| rows(&draft, cx)),
        [row("", "Stopped"), row("message", "hello")]
    );
}
