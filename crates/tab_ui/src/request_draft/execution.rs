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
    let url = request_url(&request.path);
    let templated_authorization = url.split_once("://").is_some_and(|(scheme, rest)| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority.contains("{{") || (scheme.contains("{{") && authority.contains('@'))
    });
    let templated_header_names = request.headers.iter().any(|(name, _)| name.contains("{{"));
    let mut headers =
        request::generated_headers(request.method, &url, &request.headers, body_bytes);

    if supports_body
        && request.body.is_some()
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), "application/json".into()));
    }

    if request.path.contains("{{")
        && !headers.iter().any(|(name, _)| name == "Host")
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        headers.insert(0, ("Host".into(), "Resolved on Send".into()));
    }

    if templated_authorization
        && !headers.iter().any(|(name, _)| name == "Authorization")
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    {
        headers.push(("Authorization".into(), "Resolved on Send".into()));
    }

    for (name, value) in &mut headers {
        if templated_header_names
            || (name == "Host" && value.contains("{{"))
            || (name == "Authorization" && templated_authorization)
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
    environment_error: Option<&str>,
) -> Result<HttpRequest, String> {
    let mut template = request.clone();
    if matches!(template.method, Method::Get | Method::Head) {
        template.body = None;
    }

    // Let the resolver parse names, including whitespace, in every request field.
    // Never use cached values while their source is unavailable.
    if environment_error.is_some() {
        values.environment.clear();
    }
    template
        .resolve_variables(&values)
        .map(|request| outgoing_request(&request))
        .map_err(|error| {
            if let environment::VariableError::Unknown(name) = &error
                && !name.starts_with('$')
                && let Some(message) = environment_error
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
        let (values, environment_error) = match scope.read(cx).values() {
            Ok(values) => (values, None),
            Err(error) => (environment::VariableValues::default(), Some(error)),
        };
        let request = resolve_request(&self.request, values, environment_error.as_deref());
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
