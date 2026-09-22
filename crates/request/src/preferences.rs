use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    /// Negotiate the version; a Host override differing from the URL uses HTTP/1.1.
    #[default]
    Auto,
    Http1_1,
    Http2,
}

/// Stored defaults, applied when constructing a request executor.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct RequestPreferences {
    pub http_version: HttpVersion,
    /// Total deadline, including reading the response body. Zero disables it.
    pub timeout_ms: u64,
    /// Maximum buffered response body in MiB (1,048,576 bytes). Zero is unlimited.
    pub max_response_size_mb: u64,
    pub ssl_certificate_verification: bool,
    pub follow_all_redirects: bool,
}

impl Default for RequestPreferences {
    fn default() -> Self {
        Self {
            http_version: HttpVersion::Auto,
            timeout_ms: 0,
            max_response_size_mb: 50,
            ssl_certificate_verification: false,
            follow_all_redirects: true,
        }
    }
}
