use std::{fmt, time::Duration};

use thiserror::Error;

use crate::ScriptReport;

/// A gRPC call that did not start, with the report of its Before invoke
/// script when that ran.
#[derive(Debug)]
pub struct GrpcFailure {
    pub error: GrpcError,
    pub scripts: Vec<ScriptReport>,
}

/// A gRPC call that could not start or finish with a status.
#[derive(Debug, Error)]
pub enum GrpcError {
    #[error("Enter a server URL")]
    MissingUrl,

    #[error("invalid server URL: {0}")]
    InvalidUrl(String),

    #[error("Select a method to invoke")]
    MissingMethod,

    #[error("{0} is not in the service definition")]
    UnknownMethod(String),

    #[error("{0}")]
    Variables(String),

    /// The request's authorization could not be added.
    #[error("{0}")]
    Auth(String),

    #[error("invalid metadata: {0}")]
    InvalidMetadata(String),

    #[error("invalid message: {0}")]
    InvalidMessage(String),

    #[error("the stream has ended; invoke the method again to send more messages")]
    StreamEnded,

    #[error("{0}")]
    ProtoFile(String),

    #[error("server reflection failed: {0}")]
    Reflection(String),

    #[error("could not connect to the server: {0}")]
    Connect(String),

    /// A certificate from Settings could not be used.
    #[error("{0}")]
    Certificate(String),

    #[error("the server requires TLS. Turn on TLS or use a grpcs:// URL")]
    TlsRequired,

    #[error("the server does not support TLS. Turn off TLS or use a grpc:// URL")]
    TlsUnsupported,

    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("Before invoke script failed: {message}")]
    Script { message: String },

    #[error("Call skipped: {reason}")]
    Skipped { reason: String },

    #[error("scripts could not start: {0}")]
    ScriptSetup(String),
}

/// A failure before the Before invoke script ran.
impl From<GrpcError> for GrpcFailure {
    fn from(error: GrpcError) -> Self {
        Self {
            error,
            scripts: Vec::new(),
        }
    }
}

/// The error's message, without the script's report.
impl fmt::Display for GrpcFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for GrpcFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}
