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
    /// Response bytes after decoding supported Content-Encoding values.
    /// Text encoding is left unchanged and response headers remain as received.
    pub body: Vec<u8>,
    pub metrics: HttpMetrics,
}

/// Measurements available at the HTTP client boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpMetrics {
    pub prepare: Duration,
    /// Includes connection setup, uploading the request, and waiting for headers.
    /// DNS, TCP, TLS and time to the first byte are not exposed separately.
    pub waiting: Duration,
    /// Time spent reading and decompressing the response body after its headers.
    pub download: Duration,
    /// HTTP/1-style field size estimates (`name: value\r\n`), excluding framing
    /// and compression. Request fields added by the transport are not included.
    pub request_header_bytes: usize,
    pub response_header_bytes: usize,
    pub request_body_bytes: usize,
    /// Original body size when a supported content encoding was decoded.
    /// Otherwise the downloaded size is the response body's length.
    pub encoded_response_body_bytes: Option<usize>,
}
