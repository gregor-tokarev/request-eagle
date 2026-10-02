use std::{
    convert::Infallible,
    time::{Duration, SystemTime},
};

use futures::{FutureExt as _, StreamExt as _};
use gpui_kit::*;
use preferences::Preferences;
use request::{Auth, Body, HttpRequest, Method};
use request::{EventStream, EventStreamUpdate, RequestExecutor};

use super::draft::RequestDraft;
use crate::RequestSent;
use crate::cookies::Cookies;

/// The most event-stream updates shown per redraw. A fast stream is drawn in
/// batches instead of once for every event.
const UPDATE_BATCH: usize = 512;

/// The pause after each batch, which lets the window draw and handle input
/// while a stream keeps the queue full.
const BATCH_PAUSE: Duration = Duration::from_millis(1);

fn request_url(path: &str) -> String {
    let path = path.trim();

    if !path.is_empty() && !path.contains("://") {
        format!("https://{path}")
    } else {
        path.to_owned()
    }
}

/// The headers sending adds to the request's own, including those of
/// `auth`, the authorization it sends.
pub(super) fn generated_headers(request: &HttpRequest, auth: &Auth) -> Vec<(String, String)> {
    // As sending, which leaves out empty raw text.
    let body = request
        .body
        .as_ref()
        .filter(|_| !matches!(request.method, Method::Get | Method::Head))
        .filter(|body| !matches!(body, Body::Raw { text, .. } if text.is_empty()));
    // Only raw text has its length at hand. Encoding a form or reading
    // files on each edit would be slow, and a multipart form's boundary is
    // chosen when it is sent.
    let (body_bytes, calculated, templated_body) = match body {
        Some(Body::Raw { text, .. }) => (text.len(), false, text.contains("{{")),
        // An unknown length is not zero, so the header is shown.
        Some(_) => (1, true, false),
        None => (0, false, false),
    };
    let url = request_url(&request.path);
    let templated_authorization = url.split_once("://").is_some_and(|(scheme, rest)| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority.contains("{{") || (scheme.contains("{{") && authority.contains('@'))
    });
    let templated_header_names = request.headers.iter().any(|(name, _)| name.contains("{{"));
    let mut headers =
        request::generated_headers(request.method, &url, &request.headers, body_bytes);

    if let Some(body) = body
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        let mut content_type = body.content_type();
        if matches!(body, Body::Multipart { .. }) {
            content_type.push_str("; boundary=Calculated on Send");
        }
        headers.push(("Content-Type".into(), content_type));
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
            || (name == "Content-Length" && templated_body)
        {
            *value = "Resolved on Send".into();
        } else if name == "Content-Length" && calculated {
            *value = "Calculated on Send".into();
        }
    }

    // The authorization replaces credentials written in the URL.
    let auth_headers = auth.preview_headers();
    if !auth_headers.is_empty() {
        headers.retain(|(name, _)| name != "Authorization");
    }
    let pending = if auth.kind().computes_credentials() {
        "Calculated on Send"
    } else {
        "Resolved on Send"
    };
    for (name, value) in auth_headers {
        if !request
            .headers
            .iter()
            .any(|(own, _)| own.eq_ignore_ascii_case(&name))
        {
            headers.push((name, value.unwrap_or_else(|| pending.into())));
        }
    }

    headers
}

/// The jar that requests store and send cookies in, while it is on.
pub(super) fn active_jar(cx: &App) -> Option<request::CookieJar> {
    cx.try_global::<Preferences>()
        .is_none_or(|preferences| preferences.request.cookie_jar)
        .then(|| Cookies::jar(cx))
}

/// The Cookie header that the jar adds to the request, while it is on.
fn jar_cookies(request: &HttpRequest, cx: &App) -> Option<(String, String)> {
    let jar = active_jar(cx)?;
    let url = request_url(&request.path);
    let templated = url.contains("{{")
        || request
            .path_variables
            .iter()
            .any(|(_, value)| value.contains("{{"))
        || request.headers.iter().any(|(name, value)| {
            name.contains("{{") || (name.eq_ignore_ascii_case("cookie") && value.contains("{{"))
        });

    let cookies = if templated {
        // Which cookies apply depends on the resolved URL and headers.
        (!jar.is_empty()).then(|| "Resolved on Send".to_owned())
    } else {
        // Their path decides which cookies are sent.
        let Ok(url) = request::fill_path_variables(&url, &request.path_variables, |value| {
            Ok::<_, Infallible>(value.to_owned())
        });
        jar.cookie_header(&url, &request.headers)
    };

    cookies.map(|cookies| ("Cookie".to_owned(), cookies))
}

impl RequestDraft {
    pub(super) fn refresh_generated_headers(&mut self, cx: &mut Context<Self>) {
        let mut headers = generated_headers(&self.request, &self.effective_auth());
        headers.extend(jar_cookies(&self.request, cx));

        if headers == self.generated_headers {
            return;
        }

        self.generated_headers = headers;

        if let Some(headers) = &self.headers {
            headers.update(cx, |headers, cx| {
                headers.set_generated_headers(&self.generated_headers, cx);
            });
        }

        // The Headers section counts them.
        cx.notify();
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }

        self.prepare(window, cx);
        let response = self.response.clone();
        response.update(cx, |response, cx| response.start(cx));

        let scope = self.variables.clone();
        // History keeps where the files were found, so the request can be
        // sent again from it.
        let request = self.sent_request();
        let url = request.path.clone();
        let variables = scope.read(cx).request_variables(cx);
        // History keeps the authorization that was sent, inherited or not.
        let recorded = HttpRequest {
            auth: variables.effective_auth(&request.auth),
            ..request.clone()
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
        let cookies = Cookies::jar(cx);
        let (events, mut updates, stop) = EventStream::new();
        let dispatch = events.dispatch();
        self.stop = Some(stop);
        self.sending = Some((
            RequestSent {
                record: request_history::Record::sent(recorded),
                sent_at: SystemTime::now(),
            },
            dispatch.clone(),
        ));
        let task = cx.background_executor().spawn(async move {
            let executor = match cached.map(Ok).unwrap_or_else(|| {
                RequestExecutor::new(&preferences).map(|executor| executor.with_cookie_jar(cookies))
            }) {
                Ok(executor) => executor,
                Err(error) => return (None, Err(error), None),
            };
            let result = executor.execute_streaming(request, variables, events).await;
            // What history keeps of the outcome. A request that failed
            // before it went out is left out.
            let outcome = match &result {
                Ok(execution) => Some(Ok(request_history::Response::new(execution))),
                Err(error) if dispatch.started() => Some(Err(error.message_without_url())),
                Err(_) => None,
            };

            (
                Some((preferences, executor)),
                result.map(|execution| {
                    crate::response_view::ResponseContent::new(execution).named_after(&url)
                }),
                outcome,
            )
        });

        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            // An event stream shows its events while it is open. The updates
            // end with the response body, before post-response scripts run.
            while let Some(update) = updates.next().await {
                let mut batch = vec![update];
                while batch.len() < UPDATE_BATCH
                    && let Some(Some(update)) = updates.next().now_or_never()
                {
                    batch.push(update);
                }

                if this
                    .update_in(cx, |this, window, cx| this.receive(batch, window, cx))
                    .is_err()
                {
                    return;
                }

                cx.background_executor().timer(BATCH_PAUSE).await;
            }

            // The body is complete; only scripts remain.
            let _ = this.update(cx, |this, cx| {
                if this.streaming {
                    this.streaming = false;
                    this.stop = None;
                    this.notify_address(cx);
                }
            });

            let (executor, result, outcome) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some((mut sent, _)) = this.sending.take()
                    && let Some(outcome) = outcome
                {
                    match outcome {
                        Ok(response) => sent.record.response = Some(response),
                        Err(error) => sent.record.error = Some(error),
                    }
                    cx.emit(sent);
                }

                this.executor = executor;
                this.task = None;
                this.stop = None;
                scope.update(cx, |scope, cx| scope.changed(cx));
                Cookies::changed(cx);
                // Scripts may have changed variables that other visible tabs of
                // the collection share; redraw so their chips recolor.
                window.refresh();
                response.update(cx, |response, cx| response.finish(result, window, cx));
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn receive(
        &mut self,
        updates: Vec<EventStreamUpdate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut events = Vec::new();

        for update in updates {
            match update {
                EventStreamUpdate::Opened {
                    status,
                    version,
                    headers,
                } => {
                    // The jar stored the stream's cookies with its head.
                    Cookies::changed(cx);
                    self.streaming = true;
                    self.response.update(cx, |response, cx| {
                        response.open_stream(status, version, headers, window, cx)
                    });
                    self.notify_address(cx);
                }
                EventStreamUpdate::Event(event) => events.push(event),
            }
        }

        if !events.is_empty() {
            self.response
                .update(cx, |response, cx| response.receive_events(events, cx));
        }
    }

    /// End an open event stream. The response completes with the events that
    /// arrived, and post-response scripts run.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.streaming
            && let Some(stop) = self.stop.take()
        {
            stop.stop();
            self.response
                .update(cx, |response, cx| response.stop_stream(cx));
            self.notify_address(cx);
            cx.notify();
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.task = None;
        self.stop = None;
        self.streaming = false;
        // Redirects before the cancellation may have set cookies.
        Cookies::changed(cx);

        // The server may already act on a request that went out.
        if let Some((mut sent, dispatch)) = self.sending.take()
            && dispatch.started()
        {
            sent.record.error = Some("Request cancelled".into());
            cx.emit(sent);
        }

        self.response.update(cx, |response, cx| response.cancel(cx));

        cx.notify();
    }
}
