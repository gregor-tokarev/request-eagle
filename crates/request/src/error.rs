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
    /// The message without the address the request was sent to. Once
    /// resolved, an address can hold secrets, such as a token in its query.
    pub fn message_without_url(&self) -> String {
        let message = self.to_string();
        let url = match self {
            Self::ScriptedRequest { source, .. } => return source.message_without_url(),
            Self::Transport(error) => error
                .chain()
                .find_map(|error| error.downcast_ref::<reqwest::Error>())
                .and_then(reqwest::Error::url),
            _ => None,
        };

        match url {
            Some(url) => message.replace(&format!(" for url ({url})"), ""),
            None => message,
        }
    }
}
