use std::task::Poll;

use futures::StreamExt as _;
use gpui_kit::*;
use preferences::Preferences;
use request::{GrpcClient, GrpcEvent, GrpcEvents};

use super::definition::{DefinitionState, reflected_target};
use super::draft::GrpcDraft;

/// Events handled in one update, so fast streams do not redraw per message.
const EVENT_BATCH: usize = 256;

impl GrpcDraft {
    /// A client for the current request preferences, reused until they change.
    pub(super) fn client(&mut self, cx: &App) -> GrpcClient {
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();

        match &self.client {
            Some((settings, client)) if *settings == preferences => client.clone(),
            _ => {
                let client = GrpcClient::new(&preferences);
                self.client = Some((preferences, client.clone()));
                client
            }
        }
    }

    /// The send shortcut invokes the method, or sends the composed message
    /// while a client stream is open.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.call {
            Some(call) if call.kind.streams_requests() && call.is_sending() => {
                self.send_message(window, cx)
            }
            Some(_) => {}
            None if self.invoke_when_loaded => {}
            None => self.invoke(window, cx),
        }
    }

    /// Start a call. The service definition is loaded first when the request
    /// settings changed since it was last loaded.
    pub fn invoke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.call.is_some() {
            return;
        }

        self.send_error = None;

        if self.request.method.trim().is_empty() {
            self.response.update(cx, |response, cx| {
                response.fail("Select a method to invoke".into(), cx)
            });
            self.redraw(cx);
            return;
        }

        let variables = self.variables.read(cx).request_variables(cx);
        // Variables can point reflection at another server, such as after
        // the active environment changed. Load that server's services.
        let target_changed = reflected_target(&self.request, &variables) != self.reflected_target;

        if !self.definition_is_current() || target_changed {
            if self.current_source().is_none() {
                let message = if self.request.definition.is_reflection() {
                    "Enter a server URL"
                } else {
                    "Choose a .proto file in Service definition"
                };
                self.response
                    .update(cx, |response, cx| response.fail(message.into(), cx));
            } else {
                // A failed load is retried, as Invoke is the user asking again.
                let reload =
                    target_changed || matches!(self.definition, DefinitionState::Failed(_));
                self.invoke_when_loaded = true;
                self.response.update(cx, |response, cx| response.wait(cx));
                self.load_definition(reload, window, cx);
            }

            self.redraw(cx);
            return;
        }

        let DefinitionState::Loaded(definition) = &self.definition else {
            return;
        };
        let definition = definition.clone();
        let client = self.client(cx);
        let server: SharedString = self.request.url.trim().to_owned().into();

        match client.invoke(&self.request, variables, &definition) {
            Ok((call, events)) => {
                let kind = call.kind;
                self.response
                    .update(cx, |response, cx| response.start(kind, server, window, cx));
                self.call = Some(call);
                self.call_task = Some(self.receive(events, window, cx));
            }
            Err(error) => self.response.update(cx, |response, cx| {
                response.fail(error.to_string().into(), cx)
            }),
        }

        self.redraw(cx);
    }

    /// Show call events as they arrive, until the call ends.
    fn receive(
        &mut self,
        mut events: GrpcEvents,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let mut batch = vec![event];

                while batch.len() < EVENT_BATCH
                    && let Ok(event) = events.try_recv()
                {
                    batch.push(event);
                }

                let finished = batch.iter().any(|event| {
                    matches!(event, GrpcEvent::Finished { .. } | GrpcEvent::Failed(_))
                });
                let updated = this.update_in(cx, |this, window, cx| {
                    this.response
                        .update(cx, |response, cx| response.receive(batch, window, cx));

                    // Messages redraw only the response; the draft changes
                    // when the call ends.
                    if finished {
                        this.call = None;
                        this.call_task = None;
                        this.redraw(cx);
                    }
                });

                if finished || updated.is_err() {
                    return;
                }

                // Let input and drawing run before the next queued batch.
                yield_now().await;
            }
        })
    }

    /// Send the composed message on the open stream.
    pub(super) fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.message_state(window, cx).read(cx).value();
        let Some(call) = &mut self.call else {
            return;
        };

        self.send_error = call.send(&text).err().map(|error| error.to_string().into());
        self.redraw(cx);
    }

    /// Tell the server the client has finished sending.
    pub(super) fn end_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.call {
            call.end();
        }

        self.send_error = None;
        self.redraw(cx);
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.invoke_when_loaded = false;

        // Dropping the call resets its stream on the server.
        let cancelled = self.call.take().is_some();
        self.call_task = None;
        self.response
            .update(cx, |response, cx| response.cancel(cancelled, cx));

        self.redraw(cx);
    }
}

/// Return to the executor once, so other work runs before this task goes on.
async fn yield_now() {
    let mut yielded = false;

    futures::future::poll_fn(|cx| {
        if std::mem::replace(&mut yielded, true) {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}
