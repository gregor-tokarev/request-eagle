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
    let has_script = !request.scripts.pre_request.trim().is_empty();
    if context.is_none() && !has_script && !needs_variable_expansion(&request) {
        return Ok((request, Variables::default(), Vec::new()));
    }

    smol::unblock(move || {
        let mut report = ScriptReport {
            phase: ScriptPhase::PreRequest,
            tests: Vec::new(),
            logs: Vec::new(),
            error: None,
        };
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
            ..Default::default()
        };

        let mut body_changed = false;
        if has_script {
            let input = input(&request, &variables);
            let mut body = request.body.take().map(Bytes::from);
            let (output, script_report) = run(
                &request.scripts.pre_request,
                ScriptPhase::PreRequest,
                input,
                &mut body,
                None,
                cancelled.clone(),
                network,
            );
            request.body = body.map(Vec::from);
            report = script_report;
            if cancelled.load(Ordering::Relaxed) {
                report.error = Some("Script cancelled".into());
            }

            if let Some(message) = &report.error {
                return Err(ExecutionError::Script {
                    message: message.clone(),
                    report: Box::new(report),
                });
            }

            let mut output = output.expect("successful script output");
            output.variables.session = variables.session.take();
            if let Some(session) = &output.variables.session
                && let Err(message) = session.apply(&output.environment_changes)
            {
                report.error = Some(message.into());
                return Err(ExecutionError::Script {
                    message: message.into(),
                    report: Box::new(report),
                });
            }
            if let Some(reason) = output.skip_reason {
                return Err(ExecutionError::Skipped {
                    reason,
                    report: Box::new(report),
                });
            }
            request.method = output.method;
            request.path = output.url;
            request.query = Some(output.query);
            request.headers = output.headers;
            variables = output.variables;

            body_changed = output.body_changed;
            if body_changed {
                request.body = output.body.map(String::into_bytes);
            }
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
            context.resolve_owned(request, body_changed, &mut variables.generated)
        } else {
            expand_request(&mut request, &mut variables, body_changed).map(|()| request)
        };

        let mut request = match expanded {
            Ok(request) => request,
            Err(message) => {
                let message: String = message.chars().take(4096).collect();
                if !has_script && context.is_some() {
                    return Err(ExecutionError::Variables(message));
                }
                report.error = Some(message.clone());
                return Err(ExecutionError::Script {
                    message,
                    report: Box::new(report),
                });
            }
        };

        if context.is_some() {
            request = request.prepare_for_send();
        }

        let reports = if has_script { vec![report] } else { Vec::new() };
        Ok((request, variables, reports))
    })
    .await
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
    variables: Variables,
    mut execution: Execution,
    cancelled: Arc<AtomicBool>,
    network: Option<NetworkOptions>,
) -> Execution {
    if request.scripts.post_response.trim().is_empty() {
        return execution;
    }

    smol::unblock(move || {
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
            &request.scripts.post_response,
            ScriptPhase::PostResponse,
            input,
            &mut request_body,
            Some(&mut response.body),
            cancelled.clone(),
            network,
        );
        if cancelled.load(Ordering::Relaxed) {
            report.error = Some("Script cancelled".into());
        }
        if report.error.is_none()
            && let Some(output) = output
            && let Some(session) = &variables.session
            && let Err(message) = session.apply(&output.environment_changes)
        {
            report.error = Some(message.into());
        }
        execution.scripts.push(report);
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
