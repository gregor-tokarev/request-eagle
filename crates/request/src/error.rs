use std::{io, time::Duration};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Request skipped: {reason}")]
    Skipped {
        reason: String,
        report: Box<crate::ScriptReport>,
    },

    #[error("{source}")]
    ScriptedRequest {
        source: Box<ExecutionError>,
        reports: Vec<crate::ScriptReport>,
    },

    #[error("pre-request script failed: {message}")]
    Script {
        message: String,
        report: Box<crate::ScriptReport>,
    },

    #[error("{0}")]
    Variables(String),

    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("response body exceeds the {limit_bytes}-byte limit")]
    ResponseTooLarge { limit_bytes: u64 },

    #[error("maximum response size is too large")]
    InvalidResponseLimit,

    #[error("invalid proxy settings: {0}")]
    InvalidProxy(&'static str),

    #[error("could not initialize the HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    #[error("invalid request URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("unsupported URL scheme: {0}; expected http or https")]
    UnsupportedScheme(String),

    #[error("unsupported URL scheme: {0}; expected ws or wss")]
    UnsupportedWebSocketScheme(String),

    #[error("the server did not accept the WebSocket connection: {status}")]
    WebSocketRejected {
        status: http_client::http::StatusCode,
    },

    #[error(
        "the server answered the WebSocket handshake with a Sec-WebSocket-Accept that does not match the request"
    )]
    WebSocketAccept,

    #[error("WebSocket connection failed: {0}")]
    WebSocket(#[source] tokio_tungstenite::tungstenite::Error),

    #[error("invalid HTTP request: {0}")]
    InvalidRequest(#[from] http_client::http::Error),

    #[error(
        "invalid Host header: expected a hostname and optional port (example.com:443). Use User-Agent for a client name/version"
    )]
    InvalidHost,

    #[error("multiple Host headers: keep only one, or remove them to use the URL's host")]
    MultipleHosts,

    #[error(
        "a custom Host that differs from the URL is not supported with forced HTTP/2. Choose Auto or HTTP/1.1 in request settings"
    )]
    Http2HostOverride,

    #[error("HTTP transport failed: {0:#}")]
    Transport(#[source] anyhow::Error),

    #[error("could not read the HTTP response body: {0}")]
    ReadBody(#[source] io::Error),

    #[error("could not decode the gzip response body: {0}")]
    DecodeBody(#[source] io::Error),
}

impl ExecutionError {
    /// Whether the request went out before it failed. Scripts, variables,
    /// settings or an invalid address can stop a request before it is sent.
    pub fn was_sent(&self) -> bool {
        match self {
            Self::ScriptedRequest { source, .. } => source.was_sent(),
            Self::Timeout { .. }
            | Self::ResponseTooLarge { .. }
            | Self::WebSocketRejected { .. }
            | Self::WebSocketAccept
            | Self::WebSocket(_)
            | Self::Transport(_)
            | Self::ReadBody(_)
            | Self::DecodeBody(_) => true,
            Self::Skipped { .. }
            | Self::Script { .. }
            | Self::Variables(_)
            | Self::InvalidResponseLimit
            | Self::InvalidProxy(_)
            | Self::Client(_)
            | Self::InvalidUrl(_)
            | Self::UnsupportedScheme(_)
            | Self::UnsupportedWebSocketScheme(_)
            | Self::InvalidRequest(_)
            | Self::InvalidHost
            | Self::MultipleHosts
            | Self::Http2HostOverride => false,
        }
    }
}
