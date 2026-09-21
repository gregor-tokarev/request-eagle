use gpui_kit::SharedString;
use request::{Execution, HttpResponse, Response};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const DISPLAY_LIMIT: usize = 1024 * 1024;

pub(in crate::layout) struct ResponseContent {
    pub(super) execution: Execution,
    pub(super) raw: String,
    pub(super) pretty: Option<String>,
    pub(super) language: &'static str,
    pub(super) truncated: bool,
    pub(super) processing: Duration,
    pub(super) headers: Arc<[(SharedString, SharedString)]>,
    pub(super) cookies: Arc<[(SharedString, SharedString)]>,
}

impl ResponseContent {
    /// Prepare display text off the UI thread; keep the original response intact.
    pub(in crate::layout) fn new(execution: Execution) -> Self {
        let started = Instant::now();
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

        let headers = response
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string().into(),
                    String::from_utf8_lossy(value.as_bytes())
                        .into_owned()
                        .into(),
                )
            })
            .collect::<Arc<[(SharedString, SharedString)]>>();
        let cookies = headers
            .iter()
            .filter(|(name, _)| name == "set-cookie")
            .map(|(_, value)| {
                value
                    .split_once('=')
                    .map(|(name, value)| (name.to_owned().into(), value.to_owned().into()))
                    .unwrap_or_else(|| ("set-cookie".into(), value.clone()))
            })
            .collect();

        Self {
            execution,
            raw,
            pretty,
            language,
            truncated,
            processing: started.elapsed(),
            headers,
            cookies,
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
