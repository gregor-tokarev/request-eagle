use std::time::Duration;

use http_client::http::{HeaderMap, StatusCode, Version};

/// One completed execution. HTTP error status codes are completed responses too.
#[derive(Debug)]
pub struct Execution {
    pub response: Response,
    /// Time from dispatch until the complete response has been read.
    pub elapsed: Duration,
}

#[derive(Debug)]
pub enum Response {
    Http(HttpResponse),
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub version: Version,
    /// Preserves repeated headers and non-UTF-8 header values.
    pub headers: HeaderMap,
    /// Raw bytes; no text decoding or automatic decompression is performed.
    pub body: Vec<u8>,
}
