use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use futures::{
    SinkExt as _,
    channel::{mpsc, oneshot},
    future,
};
use http_client::{
    AsyncBody,
    http::{HeaderMap, StatusCode, Version, header::CONTENT_TYPE, response::Parts},
};
use smol::io::AsyncReadExt as _;

use crate::{
    ExecutionError,
    response_encoding::{Decoder, codings},
};

/// Updates wait here until the caller reads them. A full queue stops reading
/// the response, so a slow reader slows the server down instead of filling memory.
const UPDATE_QUEUE: usize = 256;

const READ_BUFFER: usize = 16 * 1024;

/// One event, as a browser's `EventSource` would dispatch it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSentEvent {
    /// When the line that completed it arrived.
    pub time: SystemTime,
    /// The event type: `message` unless the server named another.
    pub event: String,
    pub data: String,
    /// The last event ID the stream set. It applies until the stream sets another.
    pub id: String,
}

#[derive(Debug)]
pub enum EventStreamUpdate {
    /// The response's status and headers. Its events follow until the body ends.
    Opened {
        status: StatusCode,
        version: Version,
        headers: HeaderMap,
    },
    Event(ServerSentEvent),
}

/// An event stream's updates, in the order they arrived. They end with the
/// response body, before post-response scripts run. Other responses send none.
pub type EventStreamUpdates = mpsc::Receiver<EventStreamUpdate>;

/// Ends an open event stream. The response completes with what has arrived.
pub struct StopEventStream(oneshot::Sender<()>);

impl StopEventStream {
    pub fn stop(self) {
        let _ = self.0.send(());
    }
}

/// Whether a request went out. Its scripts, variables or address can stop
/// it before then.
#[derive(Clone, Debug, Default)]
pub struct Dispatch(Arc<AtomicBool>);

impl Dispatch {
    pub fn started(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub(crate) fn start(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Follows a request while it executes: whether it went out, and the events
/// of a response that turns out to be an event stream. Pass it to
/// `RequestExecutor::execute_streaming`.
pub struct EventStream {
    updates: Option<mpsc::Sender<EventStreamUpdate>>,
    stop: oneshot::Receiver<()>,
    /// Set when the stream opens, after which the request timeout no longer applies.
    pub(crate) opened: Arc<AtomicBool>,
    pub(crate) dispatch: Dispatch,
}

impl EventStream {
    /// The stream to execute with, its updates, and the handle that stops it.
    /// Dropping the handle leaves the stream open until the server ends it.
    pub fn new() -> (Self, EventStreamUpdates, StopEventStream) {
        let (updates, receiver) = mpsc::channel(UPDATE_QUEUE);
        let (stop, stopped) = oneshot::channel();

        (
            Self {
                updates: Some(updates),
                stop: stopped,
                opened: Arc::default(),
                dispatch: Dispatch::default(),
            },
            receiver,
            StopEventStream(stop),
        )
    }

    /// Tells whether the request went out, during and after its execution.
    pub fn dispatch(&self) -> Dispatch {
        self.dispatch.clone()
    }

    /// Report the response's events while reading its body. Returns the
    /// decoded body and, when it was compressed, its encoded size.
    pub(crate) async fn read(
        &mut self,
        head: &Parts,
        mut body: AsyncBody,
        mut decoder: Decoder,
        limit_bytes: Option<u64>,
    ) -> Result<(Vec<u8>, Option<usize>), ExecutionError> {
        self.opened.store(true, Ordering::SeqCst);
        self.send(EventStreamUpdate::Opened {
            status: head.status,
            version: head.version,
            headers: head.headers.clone(),
        })
        .await;

        let mut parser = Parser::default();
        let mut buffer = vec![0; READ_BUFFER];
        let mut parsed = 0;
        let mut events = Vec::new();
        let mut encoded_bytes = 0;

        'reading: loop {
            // Check for a stop first, so a stream that always has data ready
            // can still be stopped.
            let stop = &mut self.stop;
            let read = smol::future::or(
                async {
                    stopped(stop).await;
                    None
                },
                async { Some(body.read(&mut buffer).await) },
            )
            .await;
            let Some(read) = read else { break };
            let read = read.map_err(ExecutionError::ReadBody)?;

            if read == 0 {
                decoder.finish()?;
            } else {
                encoded_bytes += read;
                if let Some(limit_bytes) = limit_bytes
                    && encoded_bytes as u64 > limit_bytes
                {
                    return Err(ExecutionError::ResponseTooLarge { limit_bytes });
                }
                decoder.decode(&buffer[..read])?;
            }

            let decoded = decoder.decoded();
            parser.push(&decoded[parsed..], SystemTime::now(), &mut events);
            parsed = decoded.len();

            for event in events.drain(..) {
                // The stream may be stopped while its reader is busy.
                if !self.send(EventStreamUpdate::Event(event)).await {
                    break 'reading;
                }
            }

            if read == 0 {
                break;
            }
        }

        // Updates end with the body, before post-response scripts run.
        self.updates = None;

        let encoded = (!decoder.is_identity()).then_some(encoded_bytes);
        Ok((decoder.into_decoded(), encoded))
    }

    /// Returns false when the stream was stopped instead.
    async fn send(&mut self, update: EventStreamUpdate) -> bool {
        let Self { updates, stop, .. } = self;
        let Some(updates) = updates else {
            return true;
        };

        let sent = smol::future::or(
            async {
                stopped(stop).await;
                None
            },
            async { Some(updates.send(update).await.is_ok()) },
        )
        .await;

        match sent {
            Some(true) => true,
            // Nobody reads the updates any more; the response still completes.
            Some(false) => {
                self.updates = None;
                true
            }
            None => false,
        }
    }
}

/// Resolves when the stream is stopped. Dropping the stop handle does not stop it.
async fn stopped(stop: &mut oneshot::Receiver<()>) {
    if stop.await.is_err() {
        future::pending::<()>().await;
    }
}

/// Whether a response is an event stream whose events can be read as they
/// arrive: one with at most one supported content coding.
pub(crate) fn decoder(headers: &HeaderMap, limit_bytes: Option<u64>) -> Option<Decoder> {
    let event_stream = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"));

    if !event_stream {
        return None;
    }

    // Unsupported and stacked codings are decoded once the body is complete.
    match codings(headers)?[..] {
        [] => Some(Decoder::new(None, limit_bytes)),
        [coding] => Some(Decoder::new(Some(coding), limit_bytes)),
        _ => None,
    }
}

/// Interprets an event stream as the HTML standard specifies, one chunk at a time.
#[derive(Default)]
pub(crate) struct Parser {
    /// The line that is still arriving.
    line: Vec<u8>,
    /// The last chunk ended with CR, so a LF that starts the next one belongs to it.
    after_cr: bool,
    /// Whether the first line was read, which may start with a byte order mark.
    started: bool,
    event: String,
    data: String,
    id: String,
}

impl Parser {
    pub(crate) fn push(
        &mut self,
        mut bytes: &[u8],
        time: SystemTime,
        events: &mut Vec<ServerSentEvent>,
    ) {
        if self.after_cr && !bytes.is_empty() {
            self.after_cr = false;
            bytes = bytes.strip_prefix(b"\n").unwrap_or(bytes);
        }

        // CR and LF never occur inside a multibyte UTF-8 sequence, so lines
        // split on bytes and decode separately.
        while let Some(end) = bytes
            .iter()
            .position(|&byte| byte == b'\r' || byte == b'\n')
        {
            self.line.extend_from_slice(&bytes[..end]);
            let line = std::mem::take(&mut self.line);
            if let Some(event) = self.line_ended(&line, time) {
                events.push(event);
            }

            bytes = match (bytes[end], bytes.get(end + 1)) {
                (b'\r', Some(b'\n')) => &bytes[end + 2..],
                (b'\r', None) => {
                    self.after_cr = true;
                    &[]
                }
                _ => &bytes[end + 1..],
            };
        }

        self.line.extend_from_slice(bytes);
    }

    fn line_ended(&mut self, line: &[u8], time: SystemTime) -> Option<ServerSentEvent> {
        let line = String::from_utf8_lossy(line);
        let line = if self.started {
            &line
        } else {
            self.started = true;
            line.strip_prefix('\u{feff}').unwrap_or(&line)
        };

        if line.is_empty() {
            return self.dispatch(time);
        }

        let (field, value) = match line.split_once(':') {
            // A comment, often sent to keep the connection open.
            Some(("", _)) => return None,
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };

        match field {
            "event" => value.clone_into(&mut self.event),
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" if !value.contains('\0') => value.clone_into(&mut self.id),
            // Retry only affects reconnecting, which requests do not do.
            _ => {}
        }

        None
    }

    fn dispatch(&mut self, time: SystemTime) -> Option<ServerSentEvent> {
        let event = std::mem::take(&mut self.event);
        let mut data = std::mem::take(&mut self.data);

        if data.is_empty() {
            return None;
        }
        data.pop();

        Some(ServerSentEvent {
            time,
            event: if event.is_empty() {
                "message".into()
            } else {
                event
            },
            data,
            id: self.id.clone(),
        })
    }
}
