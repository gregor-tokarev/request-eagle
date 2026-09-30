use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use environment::EnvironmentSession;
use serde_json::json;

use super::{ScriptPhase, ScriptReport, engine::run, variables::Variables};
use crate::{
    Execution, ExecutionError, HttpRequest, Method, RequestExecutor, RequestVariables, Response,
    variables::resolve_request,
};

/// A dropped request future also interrupts a script on the blocking pool.
pub(crate) struct Cancellation(pub Arc<AtomicBool>);

impl Cancellation {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// State the pre-request phase hands to the post-response phase.
#[derive(Debug)]
pub(crate) struct ScriptState {
    pub variables: Variables,
    pub session: Option<EnvironmentSession>,
    /// Runs before the request's own post-response script.
    pub collection_post_response: String,
}

pub(crate) async fn pre_request(
    mut request: HttpRequest,
    variables: RequestVariables,
    executor: RequestExecutor,
    cancelled: Arc<AtomicBool>,
) -> Result<(HttpRequest, ScriptState, Vec<ScriptReport>), ExecutionError> {
    let RequestVariables {
        mut values,
        session,
        collection_scripts,
        environment_error,
    } = variables;

    let collection = match collection_scripts {
        Ok(scripts) => scripts,
        Err(message) => {
            let report = ScriptReport {
                phase: ScriptPhase::PreRequest,
                collection: true,
                tests: Vec::new(),
                logs: Vec::new(),
                error: Some(message.clone()),
            };
            return Err(ExecutionError::Script {
                message,
                report: Box::new(report),
            });
        }
    };

    // Like Postman, the collection's script runs first and shares the
    // execution's variables with the request's own script.
    let scripts: Vec<(bool, String)> = [
        (true, collection.pre_request),
        (false, request.scripts.pre_request.clone()),
    ]
    .into_iter()
    .filter(|(_, source)| !source.trim().is_empty())
    .collect();
    let scripted = !scripts.is_empty();

    smol::unblock(move || {
        let mut reports = Vec::new();
        let mut state = ScriptState {
            variables: Variables {
                environment: std::mem::take(&mut values).into_iter().collect(),
                ..Default::default()
            },
            session,
            collection_post_response: collection.post_response,
        };

        let mut body_changed = false;
        for (collection, source) in scripts {
            let input = input(&request, &state.variables);
            let mut body = request.body.take().map(Bytes::from);
            let (output, mut report) = run(
                &source,
                ScriptPhase::PreRequest,
                input,
                &mut body,
                None,
                cancelled.clone(),
                &executor,
            );
            report.collection = collection;
            request.body = body.map(Vec::from);
            if cancelled.load(Ordering::Relaxed) {
                report.error = Some("Script cancelled".into());
            }

            if let Some(message) = report.error.clone() {
                return Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Script {
                        message,
                        report: Box::new(report),
                    },
                ));
            }

            let output = output.expect("successful script output");
            if let Some(session) = &state.session
                && let Err(message) = session.apply(&output.environment_changes)
            {
                report.error = Some(message.into());
                return Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Script {
                        message: message.into(),
                        report: Box::new(report),
                    },
                ));
            }
            if let Some(reason) = output.skip_reason {
                return Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Skipped {
                        reason,
                        report: Box::new(report),
                    },
                ));
            }
            request.method = output.method;
            request.path = output.url;
            request.query = Some(output.query);
            request.headers = output.headers;
            state.variables = output.variables;

            if output.body_changed {
                body_changed = true;
                request.body = output.body.map(String::into_bytes);
            }
            reports.push(report);
        }

        if matches!(request.method, Method::Get | Method::Head) {
            request.body = None;
        }

        values = state
            .variables
            .environment
            .iter()
            .chain(state.variables.values.iter())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let resolved = resolve_request(
            &values,
            environment_error.as_deref(),
            request,
            scripted,
            body_changed,
            &mut state.variables.generated,
        );

        match resolved {
            Ok(request) => Ok((request.prepare_for_send(), state, reports)),
            Err(message) => {
                let message: String = message.chars().take(4096).collect();
                let Some(mut report) = reports.pop() else {
                    return Err(ExecutionError::Variables(message));
                };
                report.error = Some(message.clone());
                Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Script {
                        message,
                        report: Box::new(report),
                    },
                ))
            }
        }
    })
    .await
}

/// Keep the reports of scripts that completed before a later one stopped the send.
fn after_earlier_scripts(mut reports: Vec<ScriptReport>, error: ExecutionError) -> ExecutionError {
    if reports.is_empty() {
        return error;
    }

    if let ExecutionError::Script { report, .. } | ExecutionError::Skipped { report, .. } = &error {
        reports.push((**report).clone());
    }

    ExecutionError::ScriptedRequest {
        source: Box::new(error),
        reports,
    }
}

pub(crate) async fn post_response(
    request: HttpRequest,
    mut request_body: Option<Bytes>,
    mut state: ScriptState,
    mut execution: Execution,
    executor: RequestExecutor,
    cancelled: Arc<AtomicBool>,
) -> Execution {
    let scripts: Vec<(bool, String)> = [
        (true, std::mem::take(&mut state.collection_post_response)),
        (false, request.scripts.post_response.clone()),
    ]
    .into_iter()
    .filter(|(_, source)| !source.trim().is_empty())
    .collect();
    if scripts.is_empty() {
        return execution;
    }

    smol::unblock(move || {
        // A failing collection script does not prevent the request's own tests.
        for (collection, source) in scripts {
            let mut input = input(&request, &state.variables);
            let Response::Http(response) = &mut execution.response;
            input["response"] = json!({
                "code": response.status.as_u16(),
                "status": response.status.canonical_reason().unwrap_or(""),
                "responseTime": execution.elapsed.as_secs_f64() * 1000.,
                "headers": response.headers.iter().map(|(key, value)| {
                    (key.as_str(), String::from_utf8_lossy(value.as_bytes()).into_owned())
                }).collect::<Vec<_>>(),
            });
            let (output, mut report) = run(
                &source,
                ScriptPhase::PostResponse,
                input,
                &mut request_body,
                Some(&mut response.body),
                cancelled.clone(),
                &executor,
            );
            report.collection = collection;
            if cancelled.load(Ordering::Relaxed) {
                report.error = Some("Script cancelled".into());
            }
            if report.error.is_none()
                && let Some(output) = output
            {
                if let Some(session) = &state.session
                    && let Err(message) = session.apply(&output.environment_changes)
                {
                    report.error = Some(message.into());
                } else {
                    state.variables = output.variables;
                }
            }
            execution.scripts.push(report);
        }
        execution
    })
    .await
}

fn input(request: &HttpRequest, variables: &Variables) -> serde_json::Value {
    json!({
        "method": request.method.as_str(),
        "url": request.path,
        "query": request.query.as_deref().unwrap_or_default(),
        "headers": request.headers,
        "variables": variables,
    })
}
