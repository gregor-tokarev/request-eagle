use std::time::{Duration, Instant, SystemTime};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use http_client::http::uri::PathAndQuery;
use prost_reflect::{DynamicMessage, MessageDescriptor};
use tonic::{Status, metadata::MetadataMap, transport::Channel};

use super::{
    GrpcError, GrpcStatus, MethodKind,
    codec::DynamicCodec,
    definition::{format_message, parse_message},
};
use crate::RequestVariables;

/// Controls a running call. Dropping it cancels the call.
pub struct GrpcCall {
    pub kind: MethodKind,
    input: MessageDescriptor,
    /// Present until the client finishes sending.
    messages: Option<UnboundedSender<DynamicMessage>>,
    events: UnboundedSender<GrpcEvent>,
    variables: RequestVariables,
    include_defaults: bool,
    pub(super) task: Option<tokio::task::AbortHandle>,
}

/// Everything that happens during a call, in order. The stream ends after
/// `Finished` or `Failed`.
pub type GrpcEvents = UnboundedReceiver<GrpcEvent>;

#[derive(Debug)]
pub enum GrpcEvent {
    /// The server's initial metadata (response headers).
    Metadata(Vec<(String, String)>),
    Sent(GrpcMessage),
    Received(GrpcMessage),
    /// The server ended the call. Statuses other than OK are included.
    Finished {
        status: GrpcStatus,
        trailers: Vec<(String, String)>,
        /// From invoking until the status arrived.
        elapsed: Duration,
    },
    /// The call ended without a status from the server.
    Failed(GrpcError),
}

#[derive(Clone, Debug)]
pub struct GrpcMessage {
    /// Indented JSON, with default values unless the request's settings
    /// leave them out.
    pub json: String,
    pub at: SystemTime,
}

impl GrpcCall {
    pub(super) fn new(
        kind: MethodKind,
        input: MessageDescriptor,
        messages: UnboundedSender<DynamicMessage>,
        events: UnboundedSender<GrpcEvent>,
        variables: RequestVariables,
        include_defaults: bool,
    ) -> Self {
        Self {
            kind,
            input,
            messages: Some(messages),
            events,
            variables,
            include_defaults,
            task: None,
        }
    }

    /// Resolve variables in a JSON message and send it. The message is
    /// validated against the method's input type first.
    pub fn send(&mut self, text: &str) -> Result<(), GrpcError> {
        let Some(messages) = &self.messages else {
            return Err(GrpcError::StreamEnded);
        };
        let text = self
            .variables
            .resolve_text(text)
            .map_err(GrpcError::Variables)?;
        let message = parse_message(&self.input, &text)?;
        let json = format_message(&message, self.include_defaults);

        messages
            .unbounded_send(message)
            .map_err(|_| GrpcError::StreamEnded)?;
        let _ = self.events.unbounded_send(GrpcEvent::Sent(GrpcMessage {
            json,
            at: SystemTime::now(),
        }));

        Ok(())
    }

    /// Tell the server the client has no more messages.
    pub fn end(&mut self) {
        self.messages = None;
    }

    pub fn is_sending(&self) -> bool {
        self.messages.is_some()
    }
}

impl Drop for GrpcCall {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

pub(super) struct CallTarget {
    pub(super) channel: Channel,
    pub(super) path: PathAndQuery,
    pub(super) output: MessageDescriptor,
    pub(super) metadata: MetadataMap,
    pub(super) max_message_bytes: usize,
    pub(super) include_defaults: bool,
}

/// Run a call of any kind: every method streams on the wire, unary methods
/// just send and receive one message.
pub(super) async fn run(
    target: CallTarget,
    outgoing: UnboundedReceiver<DynamicMessage>,
    events: &UnboundedSender<GrpcEvent>,
) -> Result<Vec<(String, String)>, Status> {
    let mut grpc = tonic::client::Grpc::new(target.channel)
        .max_decoding_message_size(target.max_message_bytes)
        .max_encoding_message_size(usize::MAX);
    grpc.ready()
        .await
        .map_err(|error| Status::unavailable(super::transport::error_chain(&error)))?;

    let mut request = tonic::Request::new(outgoing);
    *request.metadata_mut() = target.metadata;

    let response = grpc
        .streaming(request, target.path, DynamicCodec::new(target.output))
        .await?;
    let (headers, mut stream, _) = response.into_parts();
    let _ = events.unbounded_send(GrpcEvent::Metadata(metadata_pairs(headers)));

    while let Some(message) = stream.message().await? {
        let _ = events.unbounded_send(GrpcEvent::Received(GrpcMessage {
            json: format_message(&message, target.include_defaults),
            at: SystemTime::now(),
        }));
    }

    Ok(stream
        .trailers()
        .await?
        .map(metadata_pairs)
        .unwrap_or_default())
}

pub(super) fn finish(result: Result<Vec<(String, String)>, Status>, started: Instant) -> GrpcEvent {
    let (status, trailers) = match result {
        Ok(trailers) => (GrpcStatus::ok(), trailers),
        Err(status) => (
            GrpcStatus::from(&status),
            metadata_pairs(status.metadata().clone()),
        ),
    };

    GrpcEvent::Finished {
        status,
        trailers,
        elapsed: started.elapsed(),
    }
}

/// Metadata as text pairs. The status is shown on its own, so its trailers
/// are left out.
fn metadata_pairs(metadata: MetadataMap) -> Vec<(String, String)> {
    metadata
        .into_headers()
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "grpc-status" | "grpc-message"))
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}
