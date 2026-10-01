use std::time::Duration;

use futures::{FutureExt as _, StreamExt as _};
use gpui_kit::*;
use preferences::Preferences;
use request::{EventStream, EventStreamUpdate, RequestExecutor};
use request::{Field, HttpRequest, Method};

use super::draft::RequestDraft;

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
    let has = |name: &str| {
        Field::enabled(&request.headers).any(|(header, _)| header.eq_ignore_ascii_case(name))
    };
    let templated_header_names =
        Field::enabled(&request.headers).any(|(name, _)| name.contains("{{"));
    let mut headers =
        request::generated_headers(request.method, &url, &request.headers, body_bytes);

    if supports_body && request.body.is_some() && !has("content-type") {
        headers.push(("Content-Type".into(), "application/json".into()));
    }

    if request.path.contains("{{")
        && !headers.iter().any(|(name, _)| name == "Host")
        && !has("host")
    {
        headers.insert(0, ("Host".into(), "Resolved on Send".into()));
    }

    if templated_authorization
        && !headers.iter().any(|(name, _)| name == "Authorization")
        && !has("authorization")
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
        if self.task.is_some() {
            return;
        }

        self.prepare(window, cx);
        let response = self.response.clone();
        response.update(cx, |response, cx| response.start(cx));

        let scope = self.variables.clone();
        let request = self.request.clone();
        let variables = scope.read(cx).request_variables(cx);
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        let cached = self
            .executor
            .as_ref()
            .filter(|(settings, _)| settings == &preferences)
            .map(|(_, executor)| executor.clone());
        let (events, mut updates, stop) = EventStream::new();
        self.stop = Some(stop);
        let task = cx.background_executor().spawn(async move {
            let executor = match cached
                .map(Ok)
                .unwrap_or_else(|| RequestExecutor::new(&preferences))
            {
                Ok(executor) => executor,
                Err(error) => return (None, Err(error)),
            };
            let result = executor
                .execute_streaming(request, variables, events)
                .await
                .map(crate::response_view::ResponseContent::new);

            (Some((preferences, executor)), result)
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

            let (executor, result) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.executor = executor;
                this.task = None;
                this.stop = None;
                scope.update(cx, |scope, cx| scope.changed(cx));
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

        self.response.update(cx, |response, cx| response.cancel(cx));

        cx.notify();
    }
}
