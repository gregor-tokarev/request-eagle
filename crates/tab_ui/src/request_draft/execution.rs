use collection::{HttpRequest, Method};
use gpui_kit::*;
use preferences::Preferences;
use request::RequestExecutor;

use super::draft::RequestDraft;

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
    let body_bytes = if supports_body {
        request.body.as_ref().map_or(0, Vec::len)
    } else {
        0
    };
    let mut headers = request::generated_headers(
        request.method,
        &request_url(&request.path),
        &request.headers,
        body_bytes,
    );

    if supports_body
        && request.body.is_some()
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), "application/json".into()));
    }

    for (name, value) in &mut headers {
        if (name == "Host" && request.path.contains("{{"))
            || (name == "Content-Length"
                && request
                    .body
                    .as_deref()
                    .is_some_and(|body| body.windows(2).any(|bytes| bytes == b"{{")))
        {
            *value = "Resolved on Send".into();
        }
    }

    headers
}

pub(super) fn outgoing_request(request: &HttpRequest) -> HttpRequest {
    let mut request = request.clone();
    request.path = request_url(&request.path);

    if matches!(request.method, Method::Get | Method::Head) {
        request.body = None;
    } else if request.body.is_some()
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
    mut values: environment::VariableValues,
    secret_error: Option<&str>,
) -> Result<HttpRequest, String> {
    let mut template = request.clone();
    if matches!(template.method, Method::Get | Method::Head) {
        template.body = None;
    }

    // Let the resolver parse names, including whitespace, in every request field.
    // Never use cached secrets while their store is loading or unavailable.
    if secret_error.is_some() {
        values.secrets.clear();
    }
    template
        .resolve_variables(&values)
        .map(|request| outgoing_request(&request))
        .map_err(|error| {
            if let environment::VariableError::Unknown(name) = &error
                && name.starts_with("vault:")
                && let Some(message) = secret_error
            {
                return message.to_owned();
            }
            error.to_string()
        })
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

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }

        self.prepare(window, cx);
        let response = self.response.as_ref().unwrap().clone();
        response.update(cx, |response, cx| response.start(cx));

        let scope = self.variables(cx);
        let store = crate::variables::VariableStore::global(cx);
        let store = store.read(cx);
        let secret_error = if store.loading {
            Some("Secrets are still loading. Try sending again in a moment.")
        } else {
            store.secret_error.as_deref()
        };
        let request = store
            .values(&scope.read(cx).path)
            .and_then(|values| resolve_request(&self.request, values, secret_error));
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                response.update(cx, |response, cx| {
                    response.finish(Err(request::ExecutionError::Variables(error)), window, cx)
                });
                return;
            }
        };
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

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.task = None;

        if let Some(response) = &self.response {
            response.update(cx, |response, cx| response.cancel(cx));
        }

        cx.notify();
    }
}
