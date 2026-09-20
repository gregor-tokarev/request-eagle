use request::{Execution, HttpResponse, Response};

const DISPLAY_LIMIT: usize = 1024 * 1024;

pub(in crate::layout) struct ResponseContent {
    pub(super) execution: Execution,
    pub(super) raw: String,
    pub(super) pretty: Option<String>,
    pub(super) language: &'static str,
    pub(super) truncated: bool,
}

impl ResponseContent {
    /// Prepare display text off the UI thread; keep the original response intact.
    pub(in crate::layout) fn new(execution: Execution) -> Self {
        let Response::Http(response) = &execution.response;
        let mut end = response.body.len().min(DISPLAY_LIMIT);

        // Do not split a UTF-8 character at the display boundary.
        while end < response.body.len() && end > 0 && response.body[end] & 0xc0 == 0x80 {
            end -= 1;
        }

        let truncated = end < response.body.len();
        let raw = String::from_utf8_lossy(&response.body[..end]).into_owned();
        let content_type = response
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let is_json = content_type.contains("json") || raw.trim_start().starts_with(['{', '[']);
        let pretty = if !truncated && is_json {
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
        } else {
            None
        };
        let language = if is_json {
            "json"
        } else if content_type.contains("html") {
            "html"
        } else {
            "text"
        };

        Self {
            execution,
            raw,
            pretty,
            language,
            truncated,
        }
    }

    pub(super) fn http(&self) -> &HttpResponse {
        let Response::Http(response) = &self.execution.response;
        response
    }
}

pub(super) fn size_label(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}
