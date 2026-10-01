use std::task::Poll;
use std::time::SystemTime;

use futures::StreamExt as _;
use gpui_kit::*;
use preferences::Preferences;
use request::{GrpcCall, GrpcClient, GrpcError, GrpcEvent, GrpcEvents, RequestVariables};

use super::definition::{DefinitionState, reflected_target};
use super::draft::GrpcDraft;
use crate::RequestSent;

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

    /// Whether a call is starting or open, including while its definition
    /// loads or its Before invoke script runs.
    pub(super) fn is_running(&self) -> bool {
        self.call.is_some() || self.call_task.is_some() || self.invoke_when_loaded
    }

    /// The send shortcut invokes the method, or sends the composed message
    /// while a client stream is open.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.call {
            Some(call) if call.kind.streams_requests() && call.is_sending() => {
                self.send_message(window, cx)
            }
            _ if self.is_running() => {}
            _ => self.invoke(window, cx),
        }
    }

    /// Start a call. The service definition is loaded first when the request
    /// settings changed since it was last loaded.
    pub fn invoke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.call.is_some() || self.call_task.is_some() {
            return;
        }

        self.send_error = None;

        let missing = if self.request.method.trim().is_empty() {
            Some("Select a method to invoke")
        } else if self.current_source().is_none() && self.request.definition.is_reflection() {
            Some("Enter a server URL")
        } else if self.current_source().is_none() {
            Some("Choose a .proto file in Service definition")
        } else {
            None
        };

        if let Some(message) = missing {
            self.response
                .update(cx, |response, cx| response.fail(message.into(), cx));
            self.redraw(cx);
            return;
        }

        let variables = self.variables.read(cx).request_variables(cx);

        if !self.request.scripts.before_invoke.trim().is_empty() {
            self.invoke_scripted(variables, window, cx);
            return;
        }

        // Variables can point reflection at another server, such as after
        // the active environment changed. Load that server's services.
        let target_changed = reflected_target(&self.request, &variables) != self.reflected_target;

        if !self.definition_is_current() || target_changed {
            // A failed load is retried, as Invoke is the user asking again.
            let reload = target_changed || matches!(self.definition, DefinitionState::Failed(_));
            self.invoke_when_loaded = true;
            self.response.update(cx, |response, cx| {
                response.wait("Loading the service definition…".into(), cx)
            });
            self.load_definition(reload, window, cx);
            self.redraw(cx);
            return;
        }

        let DefinitionState::Loaded(definition) = &self.definition else {
            return;
        };
        let definition = definition.clone();
        let client = self.client(cx);
        let server: SharedString = self.request.url.trim().to_owned().into();
        let sent = self.sent();
        // The HTTP client scripts use starts off the main thread. Dropping
        // the task cancels the call.
        let invoke =
            cx.background_executor()
                .spawn(client.invoke(&self.request, variables, &definition));

        self.call_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = invoke.await;
            Self::follow(this, result, server, sent, cx).await;
        }));
        self.redraw(cx);
    }

    /// Run the Before invoke script first, then load the definition for the
    /// call it prepared unless the loaded one fits: reflection may need the
    /// metadata or variables the script set.
    fn invoke_scripted(
        &mut self,
        variables: RequestVariables,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = self.client(cx);
        let collection = self.collection_path();
        let source = self.current_source();
        let sent = self.sent();
        let loaded = match &self.definition {
            DefinitionState::Loaded(definition) if self.definition_is_current() => {
                Some((definition.clone(), self.reflected_target.clone()))
            }
            _ => None,
        };
        // Scripts run off the main thread. Dropping the task interrupts them.
        let prepare = cx
            .background_executor()
            .spawn(client.prepare(&self.request, variables));

        self.response.update(cx, |response, cx| {
            response.wait("Running the Before invoke script…".into(), cx)
        });
        self.call_task = Some(cx.spawn_in(window, async move |this, cx| {
            let prepared = match prepare.await {
                Ok(prepared) => prepared,
                Err(error) => return Self::follow(this, Err(error), "".into(), sent, cx).await,
            };
            let server: SharedString = prepared.request().url.trim().to_owned().into();
            let target = reflected_target(prepared.request(), prepared.variables());

            let definition = match loaded {
                Some((definition, loaded_target)) if loaded_target == target => Ok(definition),
                _ => {
                    let load = client.load_definition(
                        prepared.request(),
                        prepared.variables(),
                        collection.as_deref(),
                    );
                    let _ = this.update(cx, |this, cx| {
                        this.response.update(cx, |response, cx| {
                            response.wait("Loading the service definition…".into(), cx)
                        })
                    });
                    let result = cx.background_executor().spawn(load).await;

                    if let Ok(definition) = &result {
                        let definition = definition.clone();
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.keep_definition(definition, source, target, window, cx)
                        });
                    }

                    result
                }
            };
            // Resolving and encoding a large message takes a while.
            let result = match definition {
                Ok(definition) => {
                    cx.background_executor()
                        .spawn(async move { client.start(prepared, &definition) })
                        .await
                }
                Err(error) => Err(prepared.fail(error)),
            };

            Self::follow(this, result, server, sent, cx).await;
        }));
        self.redraw(cx);
    }

    /// The call history keeps once it starts: the request as it is now.
    fn sent(&self) -> RequestSent {
        let mut request = self.request.clone();

        // History keeps the request outside its collection, where relative
        // `.proto` paths would not resolve.
        if let Some(collection) = self.collection_path() {
            request.definition = request.definition.resolved_from(&collection);
        }

        RequestSent {
            record: request_history::Record::sent(request),
            sent_at: SystemTime::now(),
        }
    }

    /// Show the call that `result` started, or why it did not, then its
    /// events until it ends.
    async fn follow(
        this: WeakEntity<Self>,
        result: Result<(GrpcCall, GrpcEvents), GrpcError>,
        server: SharedString,
        sent: RequestSent,
        cx: &mut AsyncWindowContext,
    ) {
        let opened = this.update_in(cx, |this, window, cx| {
            let events = match result {
                Ok((call, events)) => {
                    cx.emit(sent);
                    let kind = call.kind;
                    this.response
                        .update(cx, |response, cx| response.start(kind, server, window, cx));
                    this.call = Some(call);
                    Some(events)
                }
                Err(error) => {
                    this.call_task = None;
                    this.response
                        .update(cx, |response, cx| response.fail_invoke(error, cx));
                    None
                }
            };
            this.redraw(cx);

            events
        });

        if let Ok(Some(events)) = opened {
            Self::receive(this, events, cx).await;
        }
    }

    /// Show call events as they arrive, until the call ends.
    async fn receive(this: WeakEntity<Self>, mut events: GrpcEvents, cx: &mut AsyncWindowContext) {
        while let Some(event) = events.next().await {
            let mut batch = vec![event];

            while batch.len() < EVENT_BATCH
                && let Ok(event) = events.try_recv()
            {
                batch.push(event);
            }

            let finished = batch
                .iter()
                .any(|event| matches!(event, GrpcEvent::Finished { .. } | GrpcEvent::Failed(_)));
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

        // Dropping the call resets its stream on the server, and dropping
        // its task interrupts a Before invoke script that is still running.
        let started = self.call.take().is_some();
        self.call_task = None;
        self.response
            .update(cx, |response, cx| response.cancel(started, cx));

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
