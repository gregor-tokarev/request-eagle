use std::time::{Duration, Instant, SystemTime};

use futures::{SinkExt as _, StreamExt as _};
use gpui_kit::{App, Entity, Modifiers, TestAppContext, VisualTestContext};
use request::{
    WebSocketClose, WebSocketEvent, WebSocketEventKind, WebSocketMessage, WebSocketRequest,
};
use tokio_tungstenite::tungstenite::Message;

use super::{
    WebSocketDraft,
    draft::{ConnectionState, WebSocketSection},
    message_log::{EntryKind, Filter, MAX_ENTRIES, MessageLog},
};
use crate::request_draft::tests::element_bounds;

fn draft(
    request: WebSocketRequest,
    cx: &mut TestAppContext,
) -> (Entity<WebSocketDraft>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    cx.add_window_view(|window, cx| {
        let mut draft = WebSocketDraft::new(request, None, Default::default(), None, cx);
        draft.prepare(window, cx);
        draft
    })
}

fn event(kind: WebSocketEventKind) -> WebSocketEvent {
    WebSocketEvent {
        time: SystemTime::now(),
        kind,
    }
}

fn received(text: &str) -> WebSocketEvent {
    event(WebSocketEventKind::Received(WebSocketMessage::Text(
        text.to_owned(),
    )))
}

fn sent(text: &str) -> WebSocketEvent {
    event(WebSocketEventKind::Sent(WebSocketMessage::Text(
        text.to_owned(),
    )))
}

fn log(draft: &Entity<WebSocketDraft>, cx: &mut VisualTestContext) -> Entity<MessageLog> {
    cx.read(|cx| draft.read(cx).log.clone())
}

fn push(log: &Entity<MessageLog>, events: Vec<WebSocketEvent>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| log.update(cx, |log, cx| log.push(events, cx)));
}

/// Previews of the visible rows, newest first.
fn rows(log: &Entity<MessageLog>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        let log = log.read(cx);
        (0..log.visible.len())
            .map(|row| log.row_entry(row).1.preview.to_string())
            .collect()
    })
}

async fn wait_until(cx: &mut VisualTestContext, condition: impl Fn(&App) -> bool) {
    let started = Instant::now();

    while !cx.read(|cx| condition(cx)) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for the WebSocket"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
}

/// Greet each client, then echo its text messages.
fn serve_echo() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());

    reqwest_client::runtime().spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.send(Message::text("welcome")).await.unwrap();

        while let Some(Ok(message)) = socket.next().await {
            if message.is_text() {
                socket.send(message).await.unwrap();
            }
        }
    });

    url
}

#[gpui_kit::test]
async fn messages_stream_into_the_log_while_connected(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let url = serve_echo();
    let (draft, cx) = draft(
        WebSocketRequest {
            url,
            message: "hello".into(),
            ..WebSocketRequest::default()
        },
        cx,
    );
    let log = log(&draft, cx);

    assert!(element_bounds(cx, "websocket-empty").is_some());
    let connect = element_bounds(cx, "websocket-connect").unwrap();
    cx.simulate_click(connect.center(), Modifiers::default());

    // The greeting arrives while the connection stays open.
    wait_until(cx, |cx| log.read(cx).entries.len() == 2).await;
    cx.read(|cx| assert_eq!(draft.read(cx).state, ConnectionState::Connected));
    let first = rows(&log, cx);
    assert_eq!(first[0], "welcome");
    assert!(first[1].starts_with("Connected to ws://"));
    assert!(element_bounds(cx, "websocket-message-0").is_some());

    let send = element_bounds(cx, "send-websocket-message").unwrap();
    cx.simulate_click(send.center(), Modifiers::default());
    wait_until(cx, |cx| log.read(cx).entries.len() == 4).await;
    assert_eq!(rows(&log, cx)[..2], ["hello", "hello"]);
    cx.read(|cx| {
        let log = log.read(cx);
        assert_eq!(log.row_entry(0).1.kind, EntryKind::Received);
        assert_eq!(log.row_entry(1).1.kind, EntryKind::Sent);
    });

    // Disconnecting waits for the server's acknowledgement.
    let disconnect = element_bounds(cx, "websocket-connect").unwrap();
    cx.simulate_click(disconnect.center(), Modifiers::default());
    wait_until(cx, |cx| {
        draft.read(cx).state == ConnectionState::Disconnected
    })
    .await;
    let rows = rows(&log, cx);
    assert!(
        rows[0].contains("1000 Normal Closure"),
        "unexpected close: {}",
        rows[0]
    );
}

#[gpui_kit::test]
async fn connecting_can_be_cancelled(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    // The handshake is never answered.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (draft, cx) = draft(
        WebSocketRequest {
            url: format!("ws://{}", listener.local_addr().unwrap()),
            ..WebSocketRequest::default()
        },
        cx,
    );

    let connect = element_bounds(cx, "websocket-connect").unwrap();
    cx.simulate_click(connect.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(draft.read(cx).state, ConnectionState::Connecting));

    let cancel = element_bounds(cx, "websocket-connect").unwrap();
    cx.simulate_click(cancel.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(draft.read(cx).state, ConnectionState::Disconnected));
    assert!(element_bounds(cx, "websocket-empty").is_some());
}

#[gpui_kit::test]
fn a_failed_connection_is_shown_in_the_log(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);

    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.state = ConnectionState::Connecting;
            draft.receive(
                vec![event(WebSocketEventKind::Failed(
                    request::ExecutionError::UnsupportedWebSocketScheme("ftp".into()),
                ))],
                cx,
            );
        })
    });

    cx.read(|cx| assert_eq!(draft.read(cx).state, ConnectionState::Disconnected));
    assert_eq!(
        rows(&log, cx),
        ["unsupported URL scheme: ftp; expected ws or wss"]
    );
    cx.read(|cx| assert_eq!(log.read(cx).row_entry(0).1.kind, EntryKind::Error));
}

#[gpui_kit::test]
fn the_log_keeps_the_newest_messages(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);

    push(
        &log,
        (0..MAX_ENTRIES + 10)
            .map(|index| received(&index.to_string()))
            .collect(),
        cx,
    );

    cx.read(|cx| {
        let log = log.read(cx);
        assert_eq!(log.entries.len(), MAX_ENTRIES);
        assert_eq!(log.visible.len(), MAX_ENTRIES);
        assert_eq!(log.dropped, 10);
        assert_eq!(
            log.row_entry(0).1.preview.as_ref(),
            (MAX_ENTRIES + 9).to_string()
        );
        assert_eq!(log.row_entry(MAX_ENTRIES - 1).1.preview.as_ref(), "10");
    });
    assert!(element_bounds(cx, "websocket-dropped").is_some());

    let clear = element_bounds(cx, "clear-websocket-messages").unwrap();
    cx.simulate_click(clear.center(), Modifiers::default());
    cx.read(|cx| {
        let log = log.read(cx);
        assert!(log.entries.is_empty() && log.visible.is_empty());
        assert_eq!(log.dropped, 0);
    });
}

#[gpui_kit::test]
fn filters_and_search_narrow_the_rows(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(
        &log,
        vec![
            sent(r#"{"subscribe":"prices"}"#),
            received(r#"{"price":1}"#),
            received(r#"{"PRICE":2}"#),
            received("heartbeat"),
        ],
        cx,
    );

    cx.update(|_, cx| log.update(cx, |log, cx| log.set_filter(Filter::Sent, cx)));
    assert_eq!(rows(&log, cx), [r#"{"subscribe":"prices"}"#]);

    cx.update(|_, cx| log.update(cx, |log, cx| log.set_filter(Filter::Received, cx)));
    assert_eq!(rows(&log, cx).len(), 3);

    let search = cx.read(|cx| log.read(cx).search.clone().unwrap());
    cx.update(|window, cx| search.update(cx, |search, cx| search.replace_all("price", window, cx)));
    cx.run_until_parked();
    assert_eq!(rows(&log, cx), [r#"{"PRICE":2}"#, r#"{"price":1}"#]);

    // Messages that arrive while searching are filtered too.
    push(&log, vec![received("price 3"), received("other")], cx);
    assert_eq!(rows(&log, cx)[0], "price 3");
    assert_eq!(rows(&log, cx).len(), 3);
}

#[gpui_kit::test]
fn selecting_a_message_shows_it_below_the_list(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(
        &log,
        vec![
            received(r#"{"nested":{"value":[1,2]}}"#),
            event(WebSocketEventKind::Received(WebSocketMessage::Binary(
                b"\x00binary".to_vec(),
            ))),
            event(WebSocketEventKind::Closed(WebSocketClose {
                code: Some(1001),
                reason: "restart".into(),
                by_client: false,
            })),
        ],
        cx,
    );

    assert!(rows(&log, cx)[0].ends_with("1001 Going Away: restart"));
    assert_eq!(rows(&log, cx)[1], "Binary message");

    let json = element_bounds(cx, "websocket-message-2").unwrap();
    cx.simulate_click(json.center(), Modifiers::default());
    assert!(element_bounds(cx, "websocket-message-detail").is_some());

    let copy = element_bounds(cx, "copy-websocket-message").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().unwrap(),
        r#"{"nested":{"value":[1,2]}}"#
    );

    let binary = element_bounds(cx, "websocket-message-1").unwrap();
    cx.simulate_click(binary.center(), Modifiers::default());
    let copy = element_bounds(cx, "copy-websocket-message").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    assert!(
        cx.read_from_clipboard()
            .unwrap()
            .text()
            .unwrap()
            .starts_with("00000000  00 62 69 6e 61 72 79")
    );

    let close = element_bounds(cx, "close-websocket-message").unwrap();
    cx.simulate_click(close.center(), Modifiers::default());
    assert!(element_bounds(cx, "websocket-message-detail").is_none());
}

#[gpui_kit::test]
fn params_and_headers_edit_the_request(cx: &mut TestAppContext) {
    let (draft, cx) = draft(
        WebSocketRequest {
            url: "wss://example.com/socket".into(),
            headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
            ..WebSocketRequest::default()
        },
        cx,
    );

    // The handshake adds four headers to the request's own.
    assert!(element_bounds(cx, "websocket-section-Headers").is_some());
    let headers = element_bounds(cx, "websocket-section-Headers").unwrap();
    cx.simulate_click(headers.center(), Modifiers::default());
    cx.read(|cx| assert!(draft.read(cx).section == WebSocketSection::Headers));
    assert!(element_bounds(cx, "headers-generated-row-3").is_some());

    let params = element_bounds(cx, "websocket-section-Params").unwrap();
    cx.simulate_click(params.center(), Modifiers::default());
    cx.read(|cx| {
        assert!(draft.read(cx).section == WebSocketSection::Params);
        assert!(!draft.read(cx).is_dirty());
    });

    let url = cx.read(|cx| draft.read(cx).url.clone().unwrap());
    cx.update(|window, cx| {
        url.update(cx, |url, cx| {
            url.replace_all("wss://example.com/other", window, cx)
        })
    });
    cx.read(|cx| {
        assert_eq!(draft.read(cx).request.url, "wss://example.com/other");
        assert!(draft.read(cx).is_dirty());
    });
}

#[gpui_kit::test]
fn arrow_keys_move_between_messages(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(&log, vec![received("first"), received("second")], cx);

    let newest = element_bounds(cx, "websocket-message-0").unwrap();
    cx.simulate_click(newest.center(), Modifiers::default());
    let selected = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let log = log.read(cx);
            log.selected
                .and_then(|id| log.entry_by_id(id))
                .map(|entry| entry.preview.to_string())
        })
    };
    assert_eq!(selected(cx).as_deref(), Some("second"));

    cx.simulate_keystrokes("down");
    assert_eq!(selected(cx).as_deref(), Some("first"));
    cx.simulate_keystrokes("down");
    assert_eq!(selected(cx).as_deref(), Some("first"));
    cx.simulate_keystrokes("up");
    assert_eq!(selected(cx).as_deref(), Some("second"));
    assert!(element_bounds(cx, "websocket-message-detail").is_some());

    cx.simulate_keystrokes("escape");
    assert_eq!(selected(cx), None);
    assert!(element_bounds(cx, "websocket-message-detail").is_none());

    // Escape also closes a message that the filter no longer shows.
    push(&log, vec![sent("mine")], cx);
    let mine = element_bounds(cx, "websocket-message-0").unwrap();
    cx.simulate_click(mine.center(), Modifiers::default());
    assert_eq!(selected(cx).as_deref(), Some("mine"));
    cx.update(|_, cx| log.update(cx, |log, cx| log.set_filter(Filter::Received, cx)));
    cx.simulate_keystrokes("escape");
    assert_eq!(selected(cx), None);
}

#[gpui_kit::test]
fn arrow_keys_start_from_the_rows_in_view(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(
        &log,
        (0..100).map(|index| received(&index.to_string())).collect(),
        cx,
    );

    // Focus the list without a selection, then scroll the newest rows away.
    let newest = element_bounds(cx, "websocket-message-0").unwrap();
    cx.simulate_click(newest.center(), Modifiers::default());
    cx.simulate_keystrokes("escape");
    cx.update(|_, cx| {
        log.update(cx, |log, _| {
            log.scroll.scroll_to_item(60, gpui_kit::ScrollStrategy::Top)
        })
    });
    assert!(element_bounds(cx, "websocket-messages").is_some());

    cx.simulate_keystrokes("down");
    let row = cx.read(|cx| {
        let log = log.read(cx);
        let id = log.selected.unwrap();
        log.visible.len() - 1 - log.visible.iter().position(|&row| row == id).unwrap()
    });
    assert_eq!(row, 60);
}

#[gpui_kit::test]
fn new_messages_keep_scrolled_rows_in_place(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(
        &log,
        (0..100).map(|index| received(&index.to_string())).collect(),
        cx,
    );
    let row_height = element_bounds(cx, "websocket-message-0")
        .unwrap()
        .size
        .height;

    cx.update(|_, cx| {
        log.update(cx, |log, _| {
            log.scroll.scroll_to_item(50, gpui_kit::ScrollStrategy::Top)
        })
    });
    assert!(element_bounds(cx, "websocket-messages").is_some());
    let offset = |cx: &mut VisualTestContext| {
        cx.read(|cx| log.read(cx).scroll.0.borrow().base_handle.offset().y)
    };
    let before = offset(cx);
    assert!(before < gpui_kit::px(0.));

    push(&log, vec![received("a"), received("b"), received("c")], cx);
    assert_eq!(offset(cx), before - row_height * 3.);
}

#[gpui_kit::test]
fn arrow_keys_move_from_a_partly_visible_selection(cx: &mut TestAppContext) {
    let (draft, cx) = draft(WebSocketRequest::default(), cx);
    let log = log(&draft, cx);
    push(
        &log,
        (0..100).map(|index| received(&index.to_string())).collect(),
        cx,
    );
    let row_height = element_bounds(cx, "websocket-message-0")
        .unwrap()
        .size
        .height;

    // Select row 40, then scroll so half of it is clipped at the top.
    cx.update(|window, cx| {
        log.update(cx, |log, cx| {
            let (id, _) = log.row_entry(40);
            log.select(id, window, cx);
            window.focus(&log.focus, cx);
            log.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(gpui_kit::point(gpui_kit::px(0.), -(row_height * 40.5)));
        })
    });
    assert!(element_bounds(cx, "websocket-messages").is_some());

    cx.simulate_keystrokes("up");
    let preview = cx.read(|cx| {
        let log = log.read(cx);
        log.entry_by_id(log.selected.unwrap())
            .unwrap()
            .preview
            .to_string()
    });
    // Row 39 is the newer neighbor of row 40 ("59"), which shows "60".
    assert_eq!(preview, "60");
}
