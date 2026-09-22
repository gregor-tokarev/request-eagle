use std::{io, path::Path};

use collection::{HttpRequest, Method};
use environment::{Environment, EnvironmentLoadError};
use gpui_kit::*;
use preferences::Preferences;
use request::{ExecutionError, RequestExecutor};

use super::draft::RequestDraft;

impl EventEmitter<crate::history::HistoryEntry> for RequestDraft {}

fn request_url(path: &str) -> String {
    let path = path.trim();

    if !path.is_empty() && !path.contains("://") {
        format!("https://{path}")
    } else {
        path.to_owned()
    }
}

pub(super) fn generated_headers(request: &HttpRequest) -> Vec<(String, String)> {
    let supports_body = !matches!(request.method, Method::Get | Method::Head);
    let form = request.form.as_ref().filter(|_| supports_body);
    let body_bytes = if let Some(form) = form {
        form.encoded_len().unwrap_or(0)
    } else if supports_body {
        request.body.as_ref().map_or(0, Vec::len)
    } else {
        0
    };
    let explicit: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, _)| {
            form.is_none()
                || !["content-type", "content-length", "transfer-encoding"]
                    .iter()
                    .any(|header| name.eq_ignore_ascii_case(header))
        })
        .cloned()
        .collect();
    let mut headers = request::generated_headers(
        request.method,
        &request_url(&request.path),
        &explicit,
        body_bytes,
    );

    if let Some(form) = form {
        if form.encoded_len().is_none() {
            // The multipart boundary and file sizes are determined on send.
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
        }

        headers.push(("Content-Type".into(), form.content_type().into()));
    } else if supports_body
        && request.body.is_some()
        && !explicit
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), "application/json".into()));
    }

    headers
}

pub(super) fn outgoing_request(request: &HttpRequest) -> HttpRequest {
    let mut request = request.clone();
    request.path = request_url(&request.path);

    if matches!(request.method, Method::Get | Method::Head) {
        request.body = None;
        request.form = None;
    } else if request.form.is_none()
        && request.body.is_some()
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        request
            .headers
            .push(("Content-Type".into(), "application/json".into()));
    }

    request
}

pub(super) fn resolve_request(
    request: &HttpRequest,
    environment_path: Option<&Path>,
) -> Result<HttpRequest, ExecutionError> {
    let variables = match environment_path.map(Environment::from_file) {
        Some(Ok(environment)) => environment.entries,
        Some(Err(EnvironmentLoadError::Read { source, .. }))
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Default::default()
        }
        Some(Err(error)) => return Err(ExecutionError::InvalidVariables(error.to_string())),
        None => Default::default(),
    };
    let resolved = request::resolve_variables(request, &variables)
        .map_err(|error| ExecutionError::InvalidVariables(error.to_string()))?;

    Ok(outgoing_request(&resolved))
}

impl RequestDraft {
    pub(super) fn refresh_generated_headers(&mut self, cx: &mut Context<Self>) {
        self.generated_headers = generated_headers(&self.request);

        if let Some(headers) = &self.headers {
            headers.update(cx, |headers, cx| {
                headers.set_generated_headers(&self.generated_headers, cx);
            });
        }
    }

    pub(in crate::layout) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }

        self.prepare(window, cx);
        cx.emit(crate::history::HistoryEntry::new(
            self.name.to_string(),
            self.request.clone(),
            self.environment_path.clone(),
        ));

        let response = self.response.as_ref().unwrap().clone();
        response.update(cx, |response, cx| response.start(cx));

        let request = self.request.clone();
        let environment_path = self.environment_path.clone();
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        let cached = self
            .executor
            .as_ref()
            .filter(|(settings, _)| settings == &preferences)
            .map(|(_, executor)| executor.clone());
        let task = cx.background_executor().spawn(async move {
            let request = match resolve_request(&request, environment_path.as_deref()) {
                Ok(request) => request,
                Err(error) => return (None, Err(error)),
            };
            let executor = match cached
                .map(Ok)
                .unwrap_or_else(|| RequestExecutor::new(&preferences))
            {
                Ok(executor) => executor,
                Err(error) => return (None, Err(error)),
            };
            let result = executor
                .execute(request)
                .await
                .map(super::super::response_view::ResponseContent::new);

            (Some((preferences, executor)), result)
        });

        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let (executor, result) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.executor = executor;
                this.task = None;
                response.update(cx, |response, cx| response.finish(result, window, cx));
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.task = None;

        if let Some(response) = &self.response {
            response.update(cx, |response, cx| response.cancel(cx));
        }

        cx.notify();
    }
}
