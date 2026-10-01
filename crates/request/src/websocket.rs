use std::{
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, ready},
    time::{Duration, Instant, SystemTime},
};

use futures::{
    SinkExt as _, Stream, StreamExt as _,
    channel::{mpsc, oneshot},
    future::{self, Either},
    task::AtomicWaker,
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

use crate::{
    ExecutionError, Field, Method, RequestPreferences, RequestVariables, WebSocketRequest,
};

/// Events wait in a queue until the tab reads them. When it holds this many
/// events, or this many bytes of received messages, the connection stops
/// reading, so a fast server is slowed down by TCP flow control instead of
/// filling memory. A single message may exceed the byte budget.
const EVENT_QUEUE: usize = 256;
const QUEUED_BYTES: usize = 16 * 1024 * 1024;

/// How long closing waits to send the close frame, and then for the server
/// to acknowledge it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

const KEY_HEADER: &str = "Sec-WebSocket-Key";

/// Upgrade headers the handshake adds unless the request sets them.
const UPGRADE_HEADERS: [(&str, &str); 4] = [
    ("Connection", "Upgrade"),
    ("Upgrade", "websocket"),
    ("Sec-WebSocket-Version", "13"),
    (KEY_HEADER, "Generated on connect"),
];

/// The headers a handshake adds to the request's own, which take precedence.
/// Before connecting, the key and any values from `{{variables}}` are placeholders.
pub fn websocket_handshake_headers(url: &str, headers: &[Field]) -> Vec<(String, String)> {
    let url = websocket_url(url);
    let templated = url.contains("{{");
    let has = |name: &str| Field::enabled(headers).any(|(key, _)| key.eq_ignore_ascii_case(name));

    // The HTTP client sends Host and Accept. Credentials in the URL become
    // Basic authorization, as for HTTP requests.
    let mut generated: Vec<(String, String)> =
        crate::generated_headers(Method::Get, &http_url(&url), headers, 0)
            .into_iter()
            .filter(|(name, _)| matches!(name.as_str(), "Host" | "Authorization" | "Accept"))
            .map(|(name, value)| {
                let value = if templated && name != "Accept" {
                    "Resolved on connect".to_owned()
                } else {
                    value
                };

                (name, value)
            })
            .collect();

    if templated && !has("host") && !generated.iter().any(|(name, _)| name == "Host") {
        generated.insert(0, ("Host".into(), "Resolved on connect".into()));
    }

    generated.extend(
        UPGRADE_HEADERS
            .iter()
            .filter(|(name, _)| !has(name))
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );

    generated
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
    /// The resolved URL, including query parameters but not credentials.
    pub url: String,
    /// Every header the handshake sent.
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

/// An open or opening connection. Dropping it closes the connection.
pub struct WebSocketConnection {
    messages: mpsc::UnboundedSender<(String, RequestVariables)>,
    /// Dropping the sender starts closing.
    close: Option<oneshot::Sender<()>>,
}

/// A connection's events, in the order they happened. The stream ends after
/// `Closed` or `Failed`.
pub struct WebSocketEvents {
    receiver: mpsc::Receiver<WebSocketEvent>,
    backlog: Arc<Backlog>,
}

/// Bytes of received messages waiting in the event queue.
#[derive(Default)]
struct Backlog {
    bytes: AtomicUsize,
    /// The connection's reader, while it waits for the queue to drain.
    reader: AtomicWaker,
}

impl WebSocketConnection {
    /// Start connecting in the background. Variables are resolved when
    /// connecting; each message resolves them when it is sent.
    pub fn open(
        request: WebSocketRequest,
        variables: RequestVariables,
        preferences: &RequestPreferences,
    ) -> (Self, WebSocketEvents) {
        let (messages, message_receiver) = mpsc::unbounded();
        let (close, closing) = oneshot::channel();
        let (events, receiver) = mpsc::channel(EVENT_QUEUE);
        let backlog = Arc::new(Backlog::default());
        let connection = Connection {
            messages: message_receiver,
            closing,
            events,
            backlog: backlog.clone(),
        };

        // The HTTP client performs the handshake and needs its Tokio runtime.
        reqwest_client::runtime().spawn(connection.run(request, variables, preferences.clone()));

        (
            Self {
                messages,
                close: Some(close),
            },
            WebSocketEvents { receiver, backlog },
        )
    }

    /// Queue a text message. Its `Sent` event reports the text as sent,
    /// after variables were resolved.
    pub fn send(&self, message: String, variables: RequestVariables) {
        let _ = self.messages.unbounded_send((message, variables));
    }

    /// Start a normal closure. Messages that were not sent yet are dropped.
    pub fn close(&mut self) {
        self.close = None;
    }
}

impl Stream for WebSocketEvents {
    type Item = WebSocketEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let event = ready!(self.receiver.poll_next_unpin(cx));

        if let Some(WebSocketEvent {
            kind: WebSocketEventKind::Received(message),
            ..
        }) = &event
        {
            self.backlog
                .bytes
                .fetch_sub(message_bytes(message), Ordering::SeqCst);
            self.backlog.reader.wake();
        }

        Poll::Ready(event)
    }
}

impl Drop for WebSocketEvents {
    fn drop(&mut self) {
        // Let a waiting reader find out that nothing reads the queue any more.
        self.backlog.bytes.store(0, Ordering::SeqCst);
        self.backlog.reader.wake();
    }
}

/// The background half of a connection.
struct Connection {
    messages: mpsc::UnboundedReceiver<(String, RequestVariables)>,
    /// Resolves when the owner closes or drops the connection.
    closing: oneshot::Receiver<()>,
    events: mpsc::Sender<WebSocketEvent>,
    backlog: Arc<Backlog>,
}

impl Connection {
    async fn run(
        self,
        request: WebSocketRequest,
        variables: RequestVariables,
        preferences: RequestPreferences,
    ) {
        let Self {
            mut messages,
            mut closing,
            mut events,
            backlog,
        } = self;

        let connecting = async {
            let request = variables
                .resolve_websocket(&request)
                .map_err(ExecutionError::Variables)?;

            handshake(request, &preferences).await
        };
        let connecting = pin!(async {
            match (preferences.timeout_ms != 0)
                .then(|| Duration::from_millis(preferences.timeout_ms))
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
        });

        // Closing stops a handshake that would otherwise never end.
        let (socket, handshake) = match future::select(connecting, &mut closing).await {
            Either::Left((Ok(connected), _)) => connected,
            Either::Left((Err(error), _)) => {
                let _ = events.send(event(WebSocketEventKind::Failed(error))).await;
                return;
            }
            Either::Right(_) => return,
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
                // The message may wait below; the log shows when it arrived.
                let time = SystemTime::now();
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

                future::poll_fn(|cx| {
                    backlog.reader.register(cx.waker());

                    if backlog.bytes.load(Ordering::SeqCst) < QUEUED_BYTES {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                backlog
                    .bytes
                    .fetch_add(message_bytes(&message), Ordering::SeqCst);

                let kind = WebSocketEventKind::Received(message);
                if received.send(WebSocketEvent { time, kind }).await.is_err() {
                    break;
                }
            }

            Ok::<_, tungstenite::Error>(close)
        });

        // Returns whether the close frame was sent.
        let writer = pin!(async move {
            while let Either::Left((Some((text, variables)), _)) =
                future::select(messages.next(), &mut closing).await
            {
                let kind = match variables.resolve_text(&text) {
                    Ok(text) => {
                        // A peer that stops reading can hold a send forever.
                        // Closing abandons it, without a close frame.
                        let send = sink.send(Message::text(text.clone()));
                        match future::select(send, &mut closing).await {
                            Either::Left((result, _)) => result?,
                            Either::Right(_) => return Ok(false),
                        }

                        WebSocketEventKind::Sent(WebSocketMessage::Text(text))
                    }
                    Err(error) => WebSocketEventKind::NotSent(ExecutionError::Variables(error)),
                };

                let _ = sent.send(event(kind)).await;
            }

            let close = sink.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "".into(),
            })));

            smol::future::or(async { close.await.map(|()| true) }, async {
                smol::Timer::after(CLOSE_TIMEOUT).await;
                Ok(false)
            })
            .await
        });

        let result = match future::select(reader, writer).await {
            Either::Left((read, _)) => read.map(|frame| closed(frame, false)),
            Either::Right((Ok(true), reader)) => {
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
            Either::Right((Ok(false), _)) => Ok(closed(None, true)),
            Either::Right((Err(error), _)) => Err(error),
        };

        let kind = match result {
            Ok(close) => WebSocketEventKind::Closed(close),
            Err(error) => WebSocketEventKind::Failed(ExecutionError::WebSocket(error)),
        };
        let _ = events.send(event(kind)).await;
    }
}

fn message_bytes(message: &WebSocketMessage) -> usize {
    match message {
        WebSocketMessage::Text(text) => text.len(),
        WebSocketMessage::Binary(bytes) => bytes.len(),
    }
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
    let query: Vec<_> = Field::enabled(&request.query).collect();
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }

    let mut headers = websocket_handshake_headers(url.as_str(), &request.headers);
    for (name, value) in &mut headers {
        if name == KEY_HEADER {
            *value = generate_key();
        }
    }
    headers.extend(
        Field::enabled(&request.headers).map(|(name, value)| (name.to_owned(), value.to_owned())),
    );

    // Credentials are sent as the Authorization header above instead, and
    // are not shown with the URL.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let shown_url = url.to_string();
    // The HTTP client sends the upgrade request; ws and wss share their ports.
    let _ = url.set_scheme(scheme);

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

/// The address of the HTTP request that upgrades to a WebSocket.
fn http_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else {
        url.to_owned()
    }
}
