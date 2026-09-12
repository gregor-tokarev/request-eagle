use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    #[default]
    Auto,
    Http1_1,
    Http2,
}

/// Stored request defaults. These are not yet applied to outgoing requests.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct RequestPreferences {
    pub http_version: HttpVersion,
    pub timeout_ms: u64,
    pub max_response_size_mb: u64,
    pub ssl_certificate_verification: bool,
}

impl Default for RequestPreferences {
    fn default() -> Self {
        Self {
            http_version: HttpVersion::Auto,
            timeout_ms: 0,
            max_response_size_mb: 50,
            ssl_certificate_verification: false,
        }
    }
}
