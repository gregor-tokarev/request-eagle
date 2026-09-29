use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use serde_json::json;

use super::{
    ScriptPhase, ScriptReport,
    engine::run,
    network::NetworkOptions,
    variables::{Variables, expand_request, needs_variable_expansion},
};
use crate::{Execution, ExecutionError, HttpRequest, Method, Response};

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

#[cfg(test)]
pub(super) async fn pre_request(
    request: HttpRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<(HttpRequest, Variables, Vec<ScriptReport>), ExecutionError> {
    pre_request_with_variables(request, cancelled, None).await
}

#[cfg(test)]
pub(super) async fn pre_request_with_variables(
    request: HttpRequest,
    cancelled: Arc<AtomicBool>,
    context: Option<crate::RequestVariables>,
) -> Result<(HttpRequest, Variables, Vec<ScriptReport>), ExecutionError> {
    pre_request_with_network(request, cancelled, context, None).await
}

pub(crate) async fn pre_request_with_network(
    mut request: HttpRequest,
    cancelled: Arc<AtomicBool>,
    mut context: Option<crate::RequestVariables>,
    network: Option<NetworkOptions>,
) -> Result<(HttpRequest, Variables, Vec<ScriptReport>), ExecutionError> {
    let collection = match context
        .as_mut()
        .map(|context| std::mem::replace(&mut context.collection_scripts, Ok(Default::default())))
    {
        Some(Ok(scripts)) => scripts,
        Some(Err(message)) => {
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
        None => Default::default(),
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
    let has_script = !scripts.is_empty();
    if context.is_none() && !has_script && !needs_variable_expansion(&request) {
        return Ok((request, Variables::default(), Vec::new()));
    }

    smol::unblock(move || {
        let mut reports = Vec::new();
        let mut variables = Variables {
            environment: context
                .as_mut()
                .map(|context| {
                    std::mem::take(&mut context.values.environment)
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default(),
            session: context.as_mut().and_then(|context| context.session.take()),
            collection_post_response: collection.post_response,
            ..Default::default()
        };

        let mut body_changed = false;
        for (collection, source) in scripts {
            let input = input(&request, &variables);
            let mut body = request.body.take().map(Bytes::from);
            let (output, mut report) = run(
                &source,
                ScriptPhase::PreRequest,
                input,
                &mut body,
                None,
                cancelled.clone(),
                network.clone(),
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

            let mut output = output.expect("successful script output");
            output.variables.session = variables.session.take();
            output.variables.collection_post_response =
                std::mem::take(&mut variables.collection_post_response);
            if let Some(session) = &output.variables.session
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
            variables = output.variables;

            if output.body_changed {
                body_changed = true;
                request.body = output.body.map(String::into_bytes);
            }
            reports.push(report);
        }

        if (has_script || context.is_some()) && matches!(request.method, Method::Get | Method::Head)
        {
            request.body = None;
        }

        let expanded = if let Some(context) = &mut context {
            context.values.environment = variables
                .environment
                .iter()
                .chain(variables.values.iter())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            context.resolve_owned(request, has_script, body_changed, &mut variables.generated)
        } else {
            expand_request(&mut request, &mut variables, body_changed).map(|()| request)
        };

        let mut request = match expanded {
            Ok(request) => request,
            Err(message) => {
                let message: String = message.chars().take(4096).collect();
                let mut report = match reports.pop() {
                    Some(report) => report,
                    None if context.is_some() => return Err(ExecutionError::Variables(message)),
                    None => ScriptReport {
                        phase: ScriptPhase::PreRequest,
                        collection: false,
                        tests: Vec::new(),
                        logs: Vec::new(),
                        error: None,
                    },
                };
                report.error = Some(message.clone());
                return Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Script {
                        message,
                        report: Box::new(report),
                    },
                ));
            }
        };

        if context.is_some() {
            request = request.prepare_for_send();
        }

        Ok((request, variables, reports))
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

#[cfg(test)]
pub(super) async fn post_response(
    request: HttpRequest,
    request_body: Option<Bytes>,
    variables: Variables,
    execution: Execution,
    cancelled: Arc<AtomicBool>,
) -> Execution {
    post_response_with_network(request, request_body, variables, execution, cancelled, None).await
}

pub(crate) async fn post_response_with_network(
    request: HttpRequest,
    mut request_body: Option<Bytes>,
    mut variables: Variables,
    mut execution: Execution,
    cancelled: Arc<AtomicBool>,
    network: Option<NetworkOptions>,
) -> Execution {
    let scripts: Vec<(bool, String)> = [
        (
            true,
            std::mem::take(&mut variables.collection_post_response),
        ),
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
            let mut input = input(&request, &variables);
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
                network.clone(),
            );
            report.collection = collection;
            if cancelled.load(Ordering::Relaxed) {
                report.error = Some("Script cancelled".into());
            }
            if report.error.is_none()
                && let Some(mut output) = output
            {
                if let Some(session) = &variables.session
                    && let Err(message) = session.apply(&output.environment_changes)
                {
                    report.error = Some(message.into());
                } else {
                    output.variables.session = variables.session.take();
                    variables = output.variables;
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
