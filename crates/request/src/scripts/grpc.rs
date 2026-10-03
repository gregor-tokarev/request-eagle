use std::{
    collections::{HashMap, VecDeque},
    time::UNIX_EPOCH,
};

use environment::EnvironmentSession;
use futures::{StreamExt as _, channel::mpsc::UnboundedSender};
use serde::{
    Deserialize,
    de::{DeserializeOwned, IgnoredAny},
};
use serde_json::{Value, json};

use super::{
    ScriptPhase, ScriptReport,
    engine::{ScriptOutput, run},
    runtime::Cancellation,
    variables::Variables,
};
use crate::{
    Field, GrpcError, GrpcEvent, GrpcEvents, GrpcFailure, GrpcMessage, GrpcRequest, GrpcScripts,
    RequestExecutor, RequestVariables,
};

/// After response sees the latest messages in each direction, up to this
/// much JSON.
const HISTORY_BYTES: usize = 8 * 1024 * 1024;
/// Tests and console entries shown from all of a call's On message runs.
const ON_MESSAGE_OUTPUT: usize = 500;

/// The call after its Before invoke script changed it.
#[derive(Deserialize)]
struct CallChanges {
    url: String,
    metadata: Vec<(String, String)>,
    message: String,
}

/// The scripts of one call and the `pm.variables` they share, in the order
/// they run. Dropping it interrupts a running script.
pub(crate) struct CallScripts {
    scripts: GrpcScripts,
    variables: Variables,
    session: Option<EnvironmentSession>,
    executor: RequestExecutor,
    cancellation: Cancellation,
}

impl CallScripts {
    pub fn new(
        scripts: GrpcScripts,
        variables: &RequestVariables,
        executor: RequestExecutor,
    ) -> Self {
        Self {
            scripts,
            variables: Variables {
                scopes: variables.scopes.clone(),
                ..Default::default()
            },
            session: variables.session.clone(),
            executor,
            cancellation: Cancellation::new(),
        }
    }

    /// Keep the values generated while resolving the call, so later scripts
    /// resolve `{{$name}}` to what was sent.
    pub fn keep_generated(&mut self, values: HashMap<String, String>) {
        self.variables.generated.extend(values);
    }

    /// Whether a script runs on the call's events, after it is invoked.
    pub fn follow_events(&self) -> bool {
        !self.scripts.on_message.trim().is_empty() || !self.scripts.after_response.trim().is_empty()
    }

    /// Run the Before invoke script, which can change the URL, metadata and
    /// message, and resolve the call with the variables it set.
    pub async fn before_invoke(
        &mut self,
        request: &mut GrpcRequest,
        variables: &mut RequestVariables,
    ) -> Result<Option<ScriptReport>, GrpcFailure> {
        if self.scripts.before_invoke.trim().is_empty() {
            return Ok(None);
        }

        let input = self.input(request, "before_invoke");
        let source = self.scripts.before_invoke.clone();
        let (output, mut report) = self
            .execute::<CallChanges>(ScriptPhase::BeforeInvoke, source, input)
            .await;

        if let Some(message) = report.error.clone() {
            return Err(GrpcFailure {
                error: GrpcError::Script { message },
                scripts: vec![report],
            });
        }

        let ScriptOutput {
            request: call,
            variables: values,
            changes,
            skip_reason,
            ..
        } = output.expect("successful script output");

        if let Some(session) = &self.session
            && let Err(message) = session.apply(&changes)
        {
            report.error = Some(message.into());
            return Err(GrpcFailure {
                error: GrpcError::Script {
                    message: message.into(),
                },
                scripts: vec![report],
            });
        }

        if let Some(reason) = skip_reason {
            return Err(GrpcFailure {
                error: GrpcError::Skipped { reason },
                scripts: vec![report],
            });
        }

        request.url = call.url;
        request.metadata = call.metadata.into_iter().map(Field::from).collect();
        request.message = call.message;
        self.variables = values;

        // Like a send, `{{$name}}` resolves to the value the script generated
        // or set, rather than a new one.
        variables.values.clear();
        variables.generated = self.variables.generated.clone();
        for (name, value) in self.variables.visible() {
            if name.starts_with('$') {
                variables.generated.insert(name, value);
            } else {
                variables.values.insert(name, value);
            }
        }

        Ok(Some(report))
    }

    /// Pass the call's events on, running On message after each received
    /// message and After response before the final status. `request` is the
    /// call as it was sent.
    pub async fn forward(
        mut self,
        request: GrpcRequest,
        mut events: GrpcEvents,
        output: UnboundedSender<GrpcEvent>,
    ) {
        let on_message = !self.scripts.on_message.trim().is_empty();
        let after_response = !self.scripts.after_response.trim().is_empty();
        let mut metadata = Vec::new();
        let mut sent = History::default();
        let mut received = History::default();
        let mut count = 0;
        let mut budget = OutputBudget::default();

        while let Some(event) = events.next().await {
            let message = match &event {
                GrpcEvent::Metadata(pairs) => {
                    metadata = pairs.clone();
                    None
                }
                GrpcEvent::Sent(message) => {
                    if after_response {
                        sent.push(message.clone());
                    }
                    None
                }
                GrpcEvent::Received(message) => {
                    if after_response {
                        received.push(message.clone());
                    }
                    on_message.then(|| message.clone())
                }
                GrpcEvent::Finished {
                    status,
                    trailers,
                    elapsed,
                } => {
                    if after_response {
                        let mut input = self.input(&request, "after_response");
                        input["sent"] = sent.to_json();
                        input["response"] = json!({
                            "code": status.code,
                            "status": status.name(),
                            "statusMessage": status.message,
                            "responseTime": elapsed.as_secs_f64() * 1000.,
                            "metadata": metadata,
                            "trailers": trailers,
                            "messages": received.to_json(),
                        });
                        let report = self.after_invoke(ScriptPhase::AfterResponse, input).await;
                        let _ = output.unbounded_send(GrpcEvent::Script(report));
                    }

                    let _ = output.unbounded_send(event);
                    return;
                }
                GrpcEvent::Failed(_) | GrpcEvent::Script(_) => None,
            };

            if output.unbounded_send(event).is_err() {
                return;
            }

            if let Some(message) = message {
                count += 1;
                let mut input = self.input(&request, "on_message");
                input["received"] = message_json(&message);
                let mut report = self.after_invoke(ScriptPhase::OnMessage, input).await;
                report.message = Some(count);

                if budget.admit(&mut report) {
                    let _ = output.unbounded_send(GrpcEvent::Script(report));
                }
            }
        }
    }

    /// Run a script once the call is invoked, keeping the variables it set.
    async fn after_invoke(&mut self, phase: ScriptPhase, input: Value) -> ScriptReport {
        let source = if phase == ScriptPhase::OnMessage {
            self.scripts.on_message.clone()
        } else {
            self.scripts.after_response.clone()
        };
        let (output, mut report) = self.execute::<IgnoredAny>(phase, source, input).await;

        if report.error.is_none()
            && let Some(output) = output
        {
            if let Some(session) = &self.session
                && let Err(message) = session.apply(&output.changes)
            {
                report.error = Some(message.into());
            } else {
                self.variables = output.variables;
            }
        }

        report
    }

    async fn execute<R: DeserializeOwned + Send + 'static>(
        &self,
        phase: ScriptPhase,
        source: String,
        input: Value,
    ) -> (Option<ScriptOutput<R>>, ScriptReport) {
        let executor = self.executor.clone();
        let cancelled = self.cancellation.0.clone();

        smol::unblock(move || run(&source, phase, input, &mut None, None, cancelled, &executor))
            .await
    }

    fn input(&self, request: &GrpcRequest, phase: &str) -> Value {
        json!({
            "phase": phase,
            "url": request.url,
            "methodPath": request.method,
            "metadata": Field::enabled(&request.metadata).collect::<Vec<_>>(),
            "message": request.message,
            "variables": self.variables,
        })
    }
}

/// The latest messages in one direction, up to `HISTORY_BYTES` of JSON. The
/// newest message is kept whatever its size.
#[derive(Default)]
struct History {
    messages: VecDeque<GrpcMessage>,
    bytes: usize,
}

impl History {
    fn push(&mut self, message: GrpcMessage) {
        self.bytes += message.json.len();
        self.messages.push_back(message);

        while self.bytes > HISTORY_BYTES && self.messages.len() > 1 {
            let oldest = self.messages.pop_front().unwrap();
            self.bytes -= oldest.json.len();
        }
    }

    fn to_json(&self) -> Value {
        self.messages.iter().map(message_json).collect()
    }
}

/// A message as scripts receive it: its data and when it was sent or
/// received, in milliseconds since the epoch.
fn message_json(message: &GrpcMessage) -> Value {
    json!({
        "data": serde_json::from_str::<Value>(&message.json)
            .unwrap_or_else(|_| Value::String(message.json.clone())),
        "at": message
            .at
            .duration_since(UNIX_EPOCH)
            .map_or(0., |at| at.as_secs_f64() * 1000.),
    })
}

/// What all of a call's On message runs may show together, so a long stream
/// cannot fill the results with rows.
#[derive(Default)]
struct OutputBudget {
    /// Tests and script errors.
    results: usize,
    logs: usize,
    exceeded: bool,
}

impl OutputBudget {
    /// Trim a report to what is left, and say whether it has anything to show.
    fn admit(&mut self, report: &mut ScriptReport) -> bool {
        let results = ON_MESSAGE_OUTPUT.saturating_sub(self.results);
        let logs = ON_MESSAGE_OUTPUT.saturating_sub(self.logs);
        let over = report.tests.len() + usize::from(report.error.is_some()) > results
            || report.logs.len() > logs;
        report.tests.truncate(results);
        report.logs.truncate(logs);

        if over {
            // Say so once; later runs still update variables.
            report.error = (!std::mem::replace(&mut self.exceeded, true)).then(|| {
                "On message scripts exceeded the limit of 500 tests and 500 console entries per call; later results are not shown".into()
            });
        }

        self.results += report.tests.len() + usize::from(report.error.is_some());
        self.logs += report.logs.len();

        !report.tests.is_empty() || !report.logs.is_empty() || report.error.is_some()
    }
}
