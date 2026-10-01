use std::time::SystemTime;

/// A tab sent its request. An HTTP request reports once its response
/// completes or it fails; gRPC calls and WebSocket connections when they start.
pub struct RequestSent {
    pub record: request_history::Record,
    pub sent_at: SystemTime,
}
