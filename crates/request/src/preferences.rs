use serde::{Deserialize, Serialize};

use crate::ProxyPreferences;

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
    /// Deadline through the complete response body, including pre-request scripts.
    /// Post-response scripts have their own limit. Zero disables this deadline.
    pub timeout_ms: u64,
    /// Maximum buffered response body in MiB (1,048,576 bytes). Zero is unlimited.
    pub max_response_size_mb: u64,
    /// Verify server certificates by default. False explicitly permits invalid certificates.
    pub ssl_certificate_verification: bool,
    pub proxy: ProxyPreferences,
    pub follow_all_redirects: bool,
}

impl Default for RequestPreferences {
    fn default() -> Self {
        Self {
            http_version: HttpVersion::Auto,
            timeout_ms: 0,
            max_response_size_mb: 50,
            ssl_certificate_verification: true,
            proxy: ProxyPreferences::default(),
            follow_all_redirects: true,
        }
    }
}
