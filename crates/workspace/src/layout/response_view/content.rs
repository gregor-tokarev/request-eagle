use gpui_kit::SharedString;
use request::{Execution, HttpResponse, Response};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(in crate::layout) struct ResponseContent {
    pub(super) execution: Execution,
    pub(super) raw: SharedString,
    pub(super) pretty: Option<SharedString>,
    pub(super) language: &'static str,
    pub(super) processing: Duration,
    pub(super) headers: Arc<[(SharedString, SharedString)]>,
    pub(super) cookies: Arc<[(SharedString, SharedString)]>,
}

impl ResponseContent {
    /// Prepare display text off the UI thread; keep the original response intact.
    pub(in crate::layout) fn new(execution: Execution) -> Self {
        let started = Instant::now();
        let Response::Http(response) = &execution.response;
        let raw: SharedString = String::from_utf8_lossy(&response.body).into_owned().into();
        let content_type = response
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let is_json = content_type.contains("json") || raw.trim_start().starts_with(['{', '[']);
        let pretty = if is_json {
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .map(Into::into)
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
