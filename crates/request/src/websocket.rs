use std::{
    pin::pin,
    time::{Duration, Instant, SystemTime},
};

use futures::{
    SinkExt as _, StreamExt as _,
    channel::mpsc,
    future::{self, Either},
};
use http_client::http::{HeaderMap, StatusCode};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        self, Message,
        handshake::{client::generate_key, derive_accept_key},
        protocol::{CloseFrame, Role, WebSocketConfig, frame::coding::CloseCode},
    },
};

use crate::{ExecutionError, RequestPreferences, RequestVariables, WebSocketRequest};

/// Received messages wait here until the tab reads them. When it is full the
/// connection stops reading, so a fast server is slowed down by TCP flow
/// control instead of filling memory.
const EVENT_QUEUE: usize = 256;

/// How long a closing connection waits for the server to acknowledge.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

const KEY_HEADER: &str = "Sec-WebSocket-Key";

/// Upgrade headers the handshake adds unless the request sets them.
const HANDSHAKE_HEADERS: [(&str, &str); 4] = [
    ("Connection", "Upgrade"),
    ("Upgrade", "websocket"),
    ("Sec-WebSocket-Version", "13"),
    (KEY_HEADER, "Generated on connect"),
];

/// The handshake headers a request adds to its own, for the header editor's
/// preview. The key is a placeholder until the connection generates one.
pub fn websocket_handshake_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    HANDSHAKE_HEADERS
        .iter()
        .filter(|(name, _)| {
            !headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
        })
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[derive(Debug)]
pub struct WebSocketEvent {
    /// When it happened, before the event waited to be read.
    pub time: SystemTime,
    pub kind: WebSocketEventKind,
}

#[derive(Debug)]
pub enum WebSocketEventKind {
    Connected(WebSocketHandshake),
    Sent(WebSocketMessage),
    Received(WebSocketMessage),
    /// A message was not sent, but the connection remains open.
    NotSent(ExecutionError),
    /// The last event of a connection that closed.
    Closed(WebSocketClose),
    /// The last event of a connection that could not connect, or broke.
    Failed(ExecutionError),
}

#[derive(Clone, Debug)]
pub struct WebSocketHandshake {
    /// The resolved URL, including query parameters.
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub status: StatusCode,
    pub response_headers: HeaderMap,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSocketClose {
    /// Absent when the other side sent no close frame, or no status code.
    pub code: Option<u16>,
    pub reason: String,
    /// Whether this client started closing the connection.
    pub by_client: bool,
}

enum Command {
    Send(String, RequestVariables),
    Close,
}

/// An open or opening connection. Dropping it closes the connection.
pub struct WebSocketConnection {
    commands: mpsc::UnboundedSender<Command>,
}

impl WebSocketConnection {
    /// Start connecting in the background. Events arrive in order as they
    /// happen, and the stream ends after `Closed` or `Failed`. Variables are
    /// resolved when connecting; each message resolves them when it is sent.
    pub fn open(
        request: WebSocketRequest,
        variables: RequestVariables,
        preferences: &RequestPreferences,
    ) -> (Self, mpsc::Receiver<WebSocketEvent>) {
        let (commands, command_receiver) = mpsc::unbounded();
        let (events, event_receiver) = mpsc::channel(EVENT_QUEUE);
        let preferences = preferences.clone();

        // The HTTP client performs the handshake and needs its Tokio runtime.
        reqwest_client::runtime().spawn(run(
            request,
            variables,
            preferences,
            command_receiver,
            events,
        ));

        (Self { commands }, event_receiver)
    }

    /// Queue a text message. Its `Sent` event reports the text as sent,
    /// after variables were resolved.
    pub fn send(&self, message: String, variables: RequestVariables) {
        let _ = self
            .commands
            .unbounded_send(Command::Send(message, variables));
    }

    /// Start a normal closure. Messages queued before it are sent first.
    pub fn close(&self) {
        let _ = self.commands.unbounded_send(Command::Close);
    }
}

async fn run(
    request: WebSocketRequest,
    variables: RequestVariables,
    preferences: RequestPreferences,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut events: mpsc::Sender<WebSocketEvent>,
) {
    let connecting = async {
        let request = variables
            .resolve_websocket(&request)
            .map_err(ExecutionError::Variables)?;

        handshake(request, &preferences).await
    };
    let connecting = async {
        match (preferences.timeout_ms != 0).then(|| Duration::from_millis(preferences.timeout_ms))
        {
            Some(timeout) => {
                smol::future::or(connecting, async {
                    smol::Timer::after(timeout).await;

                    Err(ExecutionError::Timeout { timeout })
                })
                .await
            }
            None => connecting.await,
        }
    };
    // Closing or dropping the connection stops a handshake that never ends.
    let cancelled = async {
        while let Some(Command::Send(..)) = commands.next().await {}
    };
    let connected = smol::future::or(async { Some(connecting.await) }, async {
        cancelled.await;
        None
    })
    .await;

    let (socket, handshake) = match connected {
        Some(Ok(connected)) => connected,
        Some(Err(error)) => {
            let _ = events.send(event(WebSocketEventKind::Failed(error))).await;
            return;
        }
        None => return,
    };

    if events
        .send(event(WebSocketEventKind::Connected(handshake)))
        .await
        .is_err()
    {
        return;
    }

    let (mut sink, mut stream) = socket.split();
    let mut received = events.clone();
    let mut sent = events.clone();

    let reader = pin!(async move {
        let mut close = None;

        while let Some(message) = stream.next().await {
            let message = match message? {
                Message::Text(text) => WebSocketMessage::Text(text.as_str().to_owned()),
                Message::Binary(bytes) => WebSocketMessage::Binary(bytes.to_vec()),
                // Tungstenite answers the close frame; the stream then ends.
                Message::Close(frame) => {
                    close = Some(frame);
                    continue;
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            };

            if received
                .send(event(WebSocketEventKind::Received(message)))
                .await
                .is_err()
            {
                break;
            }
        }

        Ok::<_, tungstenite::Error>(close)
    });

    let writer = pin!(async move {
        // The loop also ends when the connection's owner drops it.
        while let Some(Command::Send(text, variables)) = commands.next().await {
            let kind = match variables.resolve_text(&text) {
                Ok(text) => {
                    sink.send(Message::text(text.clone())).await?;

                    WebSocketEventKind::Sent(WebSocketMessage::Text(text))
                }
                Err(error) => WebSocketEventKind::NotSent(ExecutionError::Variables(error)),
            };

            let _ = sent.send(event(kind)).await;
        }

        sink.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        })))
        .await
    });

    let result = match future::select(reader, writer).await {
        Either::Left((read, _)) => read.map(|frame| closed(frame, false)),
        Either::Right((Ok(()), reader)) => {
            let acknowledged = smol::future::or(async { Some(reader.await) }, async {
                smol::Timer::after(CLOSE_TIMEOUT).await;
                None
            })
            .await;

            // The connection is closing anyway, so a server that drops it
            // without acknowledging is not an error.
            match acknowledged {
                Some(Ok(frame)) => Ok(closed(frame, true)),
                _ => Ok(closed(None, true)),
            }
        }
        Either::Right((Err(error), _)) => Err(error),
    };

    let kind = match result {
        Ok(close) => WebSocketEventKind::Closed(close),
        Err(error) => WebSocketEventKind::Failed(ExecutionError::WebSocket(error)),
    };
    let _ = events.send(event(kind)).await;
}

fn event(kind: WebSocketEventKind) -> WebSocketEvent {
    WebSocketEvent {
        time: SystemTime::now(),
        kind,
    }
}

fn closed(frame: Option<Option<CloseFrame>>, by_client: bool) -> WebSocketClose {
    let frame = frame.flatten();

    WebSocketClose {
        code: frame.as_ref().map(|frame| frame.code.into()),
        reason: frame.map_or_else(String::new, |frame| frame.reason.as_str().to_owned()),
        by_client,
    }
}

async fn handshake(
    request: WebSocketRequest,
    preferences: &RequestPreferences,
) -> Result<(WebSocketStream<reqwest::Upgraded>, WebSocketHandshake), ExecutionError> {
    let started = Instant::now();
    let mut url = url::Url::parse(&websocket_url(&request.url))?;
    let scheme = match url.scheme() {
        "ws" | "http" => "http",
        "wss" | "https" => "https",
        scheme => {
            return Err(ExecutionError::UnsupportedWebSocketScheme(
                scheme.to_owned(),
            ));
        }
    };

    // Fragments are never sent. Parameters from the editor follow the URL's own.
    url.set_fragment(None);
    if !request.query.is_empty() {
        url.query_pairs_mut().extend_pairs(&request.query);
    }

    let shown_url = url.to_string();
    // The HTTP client sends the upgrade request; ws and wss share their ports.
    let _ = url.set_scheme(scheme);

    let mut headers = websocket_handshake_headers(&request.headers);
    for (name, value) in &mut headers {
        if name == KEY_HEADER {
            *value = generate_key();
        }
    }
    headers.extend(request.headers);

    let key = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(KEY_HEADER))
        .map(|(_, value)| value.clone())
        .unwrap_or_default();

    // WebSockets upgrade HTTP/1.1 connections. Browsers do not follow
    // redirects for them, so a redirect is reported as a rejection.
    let client = crate::http::client_builder(preferences)?
        .http1_only()
        .redirect_policy(reqwest::redirect::Policy::none())
        .build()
        .map_err(ExecutionError::Client)?;

    let mut builder = client.get(url.as_str());
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder
        .send()
        .await
        .map_err(|error| ExecutionError::Transport(error.into()))?;
    let status = response.status();

    if status != StatusCode::SWITCHING_PROTOCOLS {
        return Err(ExecutionError::WebSocketRejected { status });
    }

    let accepted = response
        .headers()
        .get("sec-websocket-accept")
        .is_some_and(|accept| accept.as_bytes() == derive_accept_key(key.as_bytes()).as_bytes());
    if !accepted {
        return Err(ExecutionError::WebSocketAccept);
    }

    let response_headers = response.headers().clone();
    let upgraded = response
        .upgrade()
        .await
        .map_err(|error| ExecutionError::Transport(error.into()))?;

    let limit = match preferences.max_response_size_mb {
        0 => None,
        megabytes => {
            Some(usize::try_from(megabytes.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX))
        }
    };
    let config = WebSocketConfig::default()
        .max_message_size(limit)
        .max_frame_size(limit);
    let socket = WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await;

    Ok((
        socket,
        WebSocketHandshake {
            url: shown_url,
            request_headers: headers,
            status,
            response_headers,
            elapsed: started.elapsed(),
        },
    ))
}

/// Apply the editor's default scheme, as HTTP requests default to HTTPS.
fn websocket_url(url: &str) -> String {
    let url = url.trim();

    if url.is_empty() || url.contains("://") {
        url.to_owned()
    } else {
        format!("wss://{url}")
    }
}
