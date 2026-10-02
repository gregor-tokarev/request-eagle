use std::{
    collections::HashMap,
    future::Future,
    io::{Read as _, Write as _},
    time::Duration,
};

use futures::{SinkExt as _, StreamExt as _};
use request::{
    ExecutionError, Field, ProxyMode, RequestPreferences, RequestVariables, StatusCode,
    WebSocketClose, WebSocketConnection, WebSocketEventKind, WebSocketEvents, WebSocketMessage,
    WebSocketRequest,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

type ServerSocket = WebSocketStream<tokio::net::TcpStream>;

/// Accept one WebSocket connection and hand it to `handler` with the
/// handshake's path and `X-Token` header.
// The handshake callback returns tokio-tungstenite's own error response.
#[allow(clippy::result_large_err)]
fn serve<F, Fut>(handler: F) -> String
where
    F: FnOnce(ServerSocket, String) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());

    reqwest_client::runtime().spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let mut handshake = String::new();
        let socket =
            tokio_tungstenite::accept_hdr_async(stream, |request: &Request, response: Response| {
                let token = request
                    .headers()
                    .get("x-token")
                    .map_or("", |value| value.to_str().unwrap());
                handshake = format!("{} {token}", request.uri());

                Ok(response)
            })
            .await
            .unwrap();

        handler(socket, handshake).await;
    });

    url
}

/// Answer the handshake with a plain HTTP response.
fn serve_http(response: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }

        stream.write_all(response.as_bytes()).unwrap();
        // Keep the connection until the client is done with it.
        let _ = stream.read(&mut [0]);
    });

    url
}

fn preferences() -> RequestPreferences {
    let mut preferences = RequestPreferences {
        timeout_ms: 5_000,
        ..RequestPreferences::default()
    };
    preferences.proxy.mode = ProxyMode::Disabled;

    preferences
}

fn variables(values: &[(&str, &str)]) -> RequestVariables {
    RequestVariables::new(
        values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<HashMap<_, _>>(),
        None,
    )
}

fn open(url: String) -> (WebSocketConnection, WebSocketEvents) {
    open_with(
        WebSocketRequest {
            url,
            ..WebSocketRequest::default()
        },
        &preferences(),
    )
}

fn open_with(
    request: WebSocketRequest,
    preferences: &RequestPreferences,
) -> (WebSocketConnection, WebSocketEvents) {
    WebSocketConnection::open(request, variables(&[]), preferences)
}

async fn next(events: &mut WebSocketEvents) -> WebSocketEventKind {
    smol::future::or(
        async { events.next().await.expect("another event").kind },
        async {
            smol::Timer::after(Duration::from_secs(10)).await;
            panic!("timed out waiting for a WebSocket event");
        },
    )
    .await
}

async fn connected(events: &mut WebSocketEvents) {
    match next(events).await {
        WebSocketEventKind::Connected(handshake) => assert_eq!(handshake.status, 101),
        event => panic!("expected a connection, got {event:?}"),
    }
}

fn text(text: &str) -> WebSocketMessage {
    WebSocketMessage::Text(text.to_owned())
}

#[test]
fn messages_stream_in_both_directions_until_the_server_closes() {
    let url = serve(|mut socket, handshake| async move {
        socket.send(Message::text(handshake)).await.unwrap();

        while let Some(Ok(message)) = socket.next().await {
            match message {
                Message::Text(text) if text == "bye" => {
                    socket
                        .close(Some(CloseFrame {
                            code: CloseCode::Away,
                            reason: "done".into(),
                        }))
                        .await
                        .unwrap();
                }
                Message::Text(text) => {
                    socket
                        .send(Message::text(format!("echo {text}")))
                        .await
                        .unwrap();
                    socket
                        .send(Message::binary(vec![0, 159, 146, 150]))
                        .await
                        .unwrap();
                }
                _ => {}
            }
        }
    });
    let values = [("token", "secret"), ("room", "42")];
    let (connection, mut events) = WebSocketConnection::open(
        WebSocketRequest {
            url: format!("{url}/feed#ignored"),
            headers: vec![Field::new("X-Token", "{{token}}")],
            query: vec![Field::new("room", "{{room}}")],
            message: String::new(),
            settings: Default::default(),
        },
        variables(&values),
        &preferences(),
    );

    smol::block_on(async {
        let WebSocketEventKind::Connected(handshake) = next(&mut events).await else {
            panic!("expected a connection");
        };
        assert_eq!(handshake.status, StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(handshake.url, format!("{url}/feed?room=42"));
        assert!(
            handshake
                .request_headers
                .contains(&("X-Token".into(), "secret".into()))
        );
        assert!(
            handshake
                .request_headers
                .iter()
                .any(|(name, value)| name == "Sec-WebSocket-Key" && value.len() == 24)
        );
        assert!(
            handshake
                .response_headers
                .contains_key("sec-websocket-accept")
        );

        // The server speaks first; the message arrives while the connection stays open.
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Received(message) if message == text("/feed?room=42 secret")
        ));

        connection.send("hello {{room}}".into(), variables(&values));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Sent(message) if message == text("hello 42")
        ));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Received(message) if message == text("echo hello 42")
        ));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Received(WebSocketMessage::Binary(bytes)) if bytes == [0, 159, 146, 150]
        ));

        connection.send("bye".into(), variables(&[]));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Sent(message) if message == text("bye")
        ));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Closed(WebSocketClose { code: Some(1001), reason, by_client: false })
                if reason == "done"
        ));
        assert!(events.next().await.is_none());
    });
}

#[test]
fn a_fast_stream_arrives_complete_and_in_order() {
    const COUNT: usize = 5_000;

    let url = serve(|mut socket, _| async move {
        for index in 0..COUNT {
            socket
                .feed(Message::text(format!("tick {index}")))
                .await
                .unwrap();
        }
        socket.close(None).await.unwrap();
        while let Some(Ok(_)) = socket.next().await {}
    });
    let (_connection, mut events) = open(url);

    smol::block_on(async {
        connected(&mut events).await;

        for index in 0..COUNT {
            match next(&mut events).await {
                WebSocketEventKind::Received(message) => {
                    assert_eq!(message, text(&format!("tick {index}")))
                }
                event => panic!("expected tick {index}, got {event:?}"),
            }
        }

        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Closed(WebSocketClose {
                by_client: false,
                ..
            })
        ));
    });
}

#[test]
fn closing_from_the_client_waits_for_the_server() {
    let url = serve(|mut socket, _| async move { while let Some(Ok(_)) = socket.next().await {} });
    let (mut connection, mut events) = open(url);

    smol::block_on(async {
        connected(&mut events).await;
        connection.close();

        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Closed(WebSocketClose {
                code: Some(1000),
                by_client: true,
                ..
            })
        ));
        assert!(events.next().await.is_none());
    });
}

#[test]
fn dropping_the_connection_closes_it() {
    let (closed, closed_receiver) = std::sync::mpsc::channel();
    let url = serve(move |mut socket, _| async move {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Close(frame) = message {
                closed
                    .send(frame.map(|frame| u16::from(frame.code)))
                    .unwrap();
            }
        }
    });
    let (connection, mut events) = open(url);

    smol::block_on(connected(&mut events));
    drop(connection);

    assert_eq!(
        closed_receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap(),
        Some(1000)
    );
}

#[test]
fn a_message_with_an_unknown_variable_is_not_sent() {
    let url = serve(|mut socket, _| async move {
        while let Some(Ok(message)) = socket.next().await {
            if message.is_text() {
                socket.send(message).await.unwrap();
            }
        }
    });
    let (connection, mut events) = open(url);

    smol::block_on(async {
        connected(&mut events).await;

        connection.send("{{missing}}".into(), variables(&[]));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::NotSent(ExecutionError::Variables(error)) if error.contains("missing")
        ));

        connection.send("still open".into(), variables(&[]));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Sent(_)
        ));
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Received(message) if message == text("still open")
        ));
    });
}

#[test]
fn a_message_over_the_size_limit_fails_the_connection() {
    let url = serve(|mut socket, _| async move {
        let _ = socket.send(Message::binary(vec![0; 1024 * 1024 + 1])).await;
        while let Some(Ok(_)) = socket.next().await {}
    });
    let (_connection, mut events) = open_with(
        WebSocketRequest {
            url,
            ..WebSocketRequest::default()
        },
        &RequestPreferences {
            max_response_size_mb: 1,
            ..preferences()
        },
    );

    smol::block_on(async {
        connected(&mut events).await;

        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::WebSocket(_))
        ));
    });
}

#[test]
fn a_rejected_handshake_reports_the_status() {
    let url = serve_http("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    let (_connection, mut events) = open(url);

    smol::block_on(async {
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::WebSocketRejected { status })
                if status == StatusCode::NOT_FOUND
        ));
        assert!(events.next().await.is_none());
    });
}

#[test]
fn an_upgrade_without_a_matching_accept_key_is_rejected() {
    let url = serve_http(
        "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: wrong\r\n\r\n",
    );
    let (_connection, mut events) = open(url);

    smol::block_on(async {
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::WebSocketAccept)
        ));
    });
}

#[test]
fn an_unanswered_handshake_times_out() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (_connection, mut events) = open_with(
        WebSocketRequest {
            url: format!("ws://{}", listener.local_addr().unwrap()),
            ..WebSocketRequest::default()
        },
        &RequestPreferences {
            timeout_ms: 200,
            ..preferences()
        },
    );

    smol::block_on(async {
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::Timeout { .. })
        ));
    });
    drop(listener);
}

#[test]
fn a_connection_timeout_overrides_the_preference() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (_connection, mut events) = open_with(
        WebSocketRequest {
            url: format!("ws://{}", listener.local_addr().unwrap()),
            settings: request::WebSocketSettings {
                timeout_ms: Some(200),
                ..Default::default()
            },
            ..WebSocketRequest::default()
        },
        &RequestPreferences {
            timeout_ms: 0,
            ..preferences()
        },
    );

    smol::block_on(async {
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::Timeout { timeout }) if timeout == Duration::from_millis(200)
        ));
    });
    drop(listener);
}

#[test]
fn only_websocket_urls_connect() {
    let (_connection, mut events) = open("ftp://example.com/socket".into());

    smol::block_on(async {
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Failed(ExecutionError::UnsupportedWebSocketScheme(scheme))
                if scheme == "ftp"
        ));
    });
}

#[test]
fn dropping_a_connection_stops_its_handshake() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (connection, mut events) = open_with(
        WebSocketRequest {
            url: format!("ws://{}", listener.local_addr().unwrap()),
            ..WebSocketRequest::default()
        },
        // Without a timeout, only the owner can stop the handshake.
        &RequestPreferences {
            timeout_ms: 0,
            ..preferences()
        },
    );

    drop(connection);

    smol::block_on(async {
        assert!(events.next().await.is_none());
    });
    drop(listener);
}

#[test]
fn closing_abandons_a_send_the_peer_never_reads() {
    // The server completes the handshake and then stops reading.
    let url = serve(|socket, _| async move {
        std::future::pending::<()>().await;
        drop(socket);
    });
    let (mut connection, mut events) = open(url);

    smol::block_on(async {
        connected(&mut events).await;
        // Larger than the socket buffers, so the send cannot finish.
        connection.send("x".repeat(64 * 1024 * 1024), variables(&[]));
        smol::Timer::after(Duration::from_millis(200)).await;

        let started = std::time::Instant::now();
        connection.close();
        assert!(matches!(
            next(&mut events).await,
            WebSocketEventKind::Closed(WebSocketClose {
                code: None,
                by_client: true,
                ..
            })
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
    });
}

#[test]
fn the_handshake_reports_every_header_it_sent() {
    let url = serve(|mut socket, _| async move { while let Some(Ok(_)) = socket.next().await {} });
    let url = url.replacen("ws://", "ws://user:secret@", 1);
    let (_connection, mut events) = open(url.clone());

    smol::block_on(async {
        let WebSocketEventKind::Connected(handshake) = next(&mut events).await else {
            panic!("expected a connection");
        };
        let names: Vec<_> = handshake
            .request_headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Host",
                "Authorization",
                "Accept",
                "Connection",
                "Upgrade",
                "Sec-WebSocket-Version",
                "Sec-WebSocket-Key"
            ]
        );
        assert!(
            handshake
                .request_headers
                .contains(&("Authorization".into(), "Basic dXNlcjpzZWNyZXQ=".into()))
        );
        // Credentials travel in the header, not in the URL shown in the log.
        assert!(!handshake.url.contains("secret"), "{}", handshake.url);
    });
}

#[test]
fn the_editor_previews_handshake_headers() {
    let preview = request::websocket_handshake_headers(
        "example.com/socket",
        &[Field::new("Upgrade", "websocket")],
    );
    assert_eq!(
        preview,
        [
            ("Host".into(), "example.com".into()),
            ("Accept".into(), "*/*".into()),
            ("Connection".into(), "Upgrade".into()),
            ("Sec-WebSocket-Version".into(), "13".into()),
            ("Sec-WebSocket-Key".into(), "Generated on connect".into()),
        ]
    );

    let templated = request::websocket_handshake_headers("wss://{{host}}/socket", &[]);
    assert_eq!(templated[0], ("Host".into(), "Resolved on connect".into()));
}
