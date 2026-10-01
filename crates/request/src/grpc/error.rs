use std::time::Duration;

use thiserror::Error;

use crate::ScriptReport;

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

    #[error("the server requires TLS. Turn on TLS or use a grpcs:// URL")]
    TlsRequired,

    #[error("the server does not support TLS. Turn off TLS or use a grpc:// URL")]
    TlsUnsupported,

    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("Before invoke script failed: {message}")]
    Script {
        message: String,
        report: Box<ScriptReport>,
    },

    #[error("Call skipped: {reason}")]
    Skipped {
        reason: String,
        report: Box<ScriptReport>,
    },

    #[error("scripts could not start: {0}")]
    ScriptSetup(String),

    /// The call did not start after its Before invoke script ran, whose
    /// results it keeps.
    #[error("{source}")]
    ScriptedCall {
        source: Box<GrpcError>,
        report: Box<ScriptReport>,
    },
}
