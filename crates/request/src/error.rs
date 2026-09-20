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
    #[error("could not initialize the HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    #[error("invalid request URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("unsupported URL scheme: {0}; expected http or https")]
    UnsupportedScheme(String),

    #[error("invalid HTTP request: {0}")]
    InvalidRequest(#[from] http_client::http::Error),

    #[error("HTTP transport failed: {0:#}")]
    Transport(#[source] anyhow::Error),

    #[error("could not read the HTTP response body: {0}")]
    ReadBody(#[source] io::Error),
}
