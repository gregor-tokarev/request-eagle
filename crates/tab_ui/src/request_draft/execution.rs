use collection::{HttpRequest, Method};
use gpui_kit::component::{WindowExt, dialog::DialogButtonProps};
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
        if self.task.is_some() || self.script_trust_prompt_open {
            return;
        }

        if !self.request.scripts.is_empty()
            && self.trusted_scripts.as_ref() != Some(&self.request.scripts)
        {
            self.script_trust_prompt_open = true;
            let scripts = self.request.scripts.clone();
            let draft = cx.entity().downgrade();
            window.open_dialog(cx, move |dialog, window, _| {
                let accept = draft.clone();
                let close = draft.clone();
                let review = draft.clone();
                let scripts = scripts.clone();

                dialog
                    .title("Run scripts for this request?")
                    .w(rems(32.).to_pixels(window.rem_size()))
                    .overlay_closable(false)
                    .child(div().text_sm().child("Scripts can read request and response data, change the destination, and send headers and body data to another server. Only run scripts you trust. Approval applies to these scripts in this tab. Cancel to review them."))
                    .button_props(DialogButtonProps::default().ok_text("Trust and Send").show_cancel(true))
                    .on_ok(move |_, window, cx| {
                        let _ = accept.update(cx, |draft, cx| {
                            if draft.request.scripts == scripts {
                                draft.trusted_scripts = Some(scripts.clone());
                                draft.script_trust_prompt_open = false;
                                draft.send(window, cx);
                            }
                        });
                        true
                    })
                    .on_cancel(move |_, window, cx| {
                        let _ = review.update(cx, |draft, cx| {
                            draft.section = super::draft::RequestSection::Scripts;
                            draft.script_phase = if draft.request.scripts.pre_request.is_empty() {
                                request::ScriptPhase::PostResponse
                            } else {
                                request::ScriptPhase::PreRequest
                            };
                            draft.prepare(window, cx);
                            cx.notify();
                        });
                        true
                    })
                    .on_close(move |_, _, cx| {
                        let _ = close.update(cx, |draft, cx| {
                            draft.script_trust_prompt_open = false;
                            cx.notify();
                        });
                    })
            });
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
        let mut request = self.request.clone();
        if matches!(request.method, Method::Get | Method::Head) {
            request.body = None;
        }
        let variables = request::RequestVariables::new(values, environment_error);
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
                .execute_with_variables(request, variables)
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
