use std::time::SystemTime;

use request_history::{Outcome, Record};
use uuid::Uuid;

/// A tab sent its request. An HTTP request reports once its response
/// completes or it fails; gRPC calls and WebSocket connections when they
/// start, and with [`RequestEnded`] when they end.
pub struct RequestSent {
    /// The id history keeps the request under.
    pub id: Uuid,
    pub record: Record,
    pub sent_at: SystemTime,
    /// Whether the request was saved in a collection when it was sent.
    pub saved: bool,
}

impl RequestSent {
    /// A request sent now.
    pub(crate) fn new(record: Record, saved: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            record,
            sent_at: SystemTime::now(),
            saved,
        }
    }
}

/// A gRPC call or WebSocket connection that history kept when it started
/// has ended.
pub struct RequestEnded {
    /// The [`RequestSent::id`] it started with.
    pub id: Uuid,
    pub outcome: Outcome,
}
