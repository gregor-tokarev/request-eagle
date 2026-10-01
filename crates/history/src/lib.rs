//! Requests as they were sent, with the responses that came back, kept on
//! disk so they can be opened again.

mod history;
mod record;

#[cfg(test)]
mod tests;

pub use history::{Entry, History, HistoryError, LIMIT};
pub use record::{BODY_LIMIT, Record, Response};
