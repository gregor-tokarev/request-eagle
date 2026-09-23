use std::{io, time::Duration};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("response body exceeds the {limit_bytes}-byte limit")]
    ResponseTooLarge { limit_bytes: u64 },

    #[error("maximum response size is too large")]
    InvalidResponseLimit,

    #[error(transparent)]
    Http(#[from] HttpError),
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("invalid proxy settings: {0}")]
    InvalidProxy(&'static str),

    #[error("could not initialize the HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    #[error("invalid request URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("unsupported URL scheme: {0}; expected http or https")]
    UnsupportedScheme(String),

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
