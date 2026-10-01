use gpui_kit::SharedString;
use request::{Execution, HttpResponse, Response};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub struct ResponseContent {
    pub(super) execution: Execution,
    pub(super) raw: SharedString,
    pub(super) pretty: Option<SharedString>,
    pub(super) raw_only: bool,
    pub(super) language: &'static str,
    pub(super) processing: Duration,
    pub(super) headers: Arc<[(SharedString, SharedString)]>,
    pub(super) cookies: Arc<[(SharedString, SharedString)]>,
    /// The size of a body that history did not keep.
    pub(super) omitted_body: Option<usize>,
}

impl ResponseContent {
    /// Prepare display text off the UI thread; keep the original response intact.
    pub fn new(execution: Execution) -> Self {
        let started = Instant::now();
        let Response::Http(response) = &execution.response;
        let raw: SharedString = String::from_utf8_lossy(&response.body).into_owned().into();
        let content_type = response
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let is_json = content_type.contains("json") || raw.trim_start().starts_with(['{', '[']);
        let mut raw_only = exceeds_editor_limit(&raw);
        let mut pretty: Option<SharedString> = if is_json && !raw_only {
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .map(Into::into)
        } else {
            None
        };
        if pretty
            .as_ref()
            .is_some_and(|text| exceeds_editor_limit(text))
        {
            raw_only = true;
            pretty = None;
        }
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
            raw_only,
            language,
            processing: started.elapsed(),
            headers,
            cookies,
            omitted_body: None,
        }
    }

    /// A response kept in history, to show again.
    pub fn recorded(response: request_history::Response) -> Self {
        let omitted_body = response.body.is_none().then_some(response.body_size);

        Self {
            omitted_body,
            ..Self::new(response.into_execution())
        }
    }

    pub(super) fn http(&self) -> &HttpResponse {
        let Response::Http(response) = &self.execution.response;
        response
    }
}

/// Whether text is too large for the highlighted editor to stay responsive.
pub(crate) fn exceeds_editor_limit(text: &str) -> bool {
    text.len() > 256 * 1024 || text.split('\n').any(|line| line.len() > 32 * 1024)
}
