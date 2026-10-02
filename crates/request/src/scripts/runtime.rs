use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use environment::EnvironmentSession;
use serde::Deserialize;
use serde_json::json;

use super::{
    ExecutionInfo, LocalVariables, ScriptPhase, ScriptReport,
    engine::{NextRequestOutput, run},
    variables::Variables,
};
use crate::{
    Body, Execution, ExecutionError, Field, FormPart, HttpRequest, Method, RequestExecutor,
    RequestVariables, Response,
    variables::{resolve_request, sent_url},
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

/// The HTTP request after a pre-request script changed it.
#[derive(Deserialize)]
struct HttpChanges {
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    /// The raw text the script set, which makes the body raw.
    body: Option<String>,
    body_changed: bool,
    /// The fields of a URL-encoded or multipart form after the script.
    fields: Option<Vec<(String, String)>>,
    parts: Option<Vec<FormPart>>,
}

/// State the pre-request phase hands to the post-response phase.
#[derive(Debug)]
pub(crate) struct ScriptState {
    pub variables: Variables,
    pub session: Option<EnvironmentSession>,
    /// Runs before the request's own post-response script.
    pub collection_post_response: String,
    /// Where the response came from, after redirects.
    pub response_url: Option<String>,
    pub info: ExecutionInfo,
    /// Where `pm.variables` carry over to the next request of a run.
    pub locals: Option<LocalVariables>,
}

pub(crate) async fn pre_request(
    mut request: HttpRequest,
    variables: RequestVariables,
    executor: RequestExecutor,
    cancelled: Arc<AtomicBool>,
) -> Result<(HttpRequest, ScriptState, Vec<ScriptReport>), ExecutionError> {
    let RequestVariables {
        scopes,
        session,
        collection_scripts,
        environment_error,
        iteration_data,
        info,
        locals,
        ..
    } = variables;

    let collection = match collection_scripts {
        Ok(scripts) => scripts,
        Err(message) => {
            let report = ScriptReport {
                phase: ScriptPhase::PreRequest,
                collection: true,
                message: None,
                tests: Vec::new(),
                logs: Vec::new(),
                error: Some(message.clone()),
                next_request: None,
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
                values: locals.as_ref().map(LocalVariables::get).unwrap_or_default(),
                scopes,
                data: iteration_data,
                ..Default::default()
            },
            session,
            collection_post_response: collection.post_response,
            response_url: None,
            info,
            locals,
        };

        // Scripts have no access to files, so they can keep or remove the
        // files the request attaches but not attach others.
        let attached: Vec<String> = match &request.body {
            Some(Body::Multipart { parts }) => parts
                .iter()
                .filter(|part| part.file)
                .map(|part| part.value.clone())
                .collect(),
            _ => Vec::new(),
        };

        let mut body_changed = false;
        for (collection, source) in scripts {
            let input = input(&request, &state, "prerequest");
            // Scripts read raw text only when they ask for it.
            let mut text = match &mut request.body {
                Some(Body::Raw { text, .. }) => Some(Bytes::from(std::mem::take(text))),
                _ => None,
            };
            let (output, mut report) = run::<HttpChanges>(
                &source,
                ScriptPhase::PreRequest,
                input,
                &mut text,
                None,
                cancelled.clone(),
                &executor,
            );
            report.collection = collection;
            if let (Some(Body::Raw { text: raw, .. }), Some(text)) = (&mut request.body, text) {
                *raw = String::from_utf8(Vec::from(text)).expect("the text read from the body");
            }
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
            report.next_request = output
                .next_request
                .take()
                .map(NextRequestOutput::into_next_request);
            if let Some(session) = &state.session
                && let Err(message) = session.apply(&output.changes)
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
                // Skipping is not failing: the script's pm.variables last.
                if let Some(locals) = &state.locals {
                    locals.set(output.variables.values.clone());
                }
                return Err(after_earlier_scripts(
                    reports,
                    ExecutionError::Skipped {
                        reason,
                        report: Box::new(report),
                    },
                ));
            }
            let changes = output.request;
            request.method = changes.method;
            request.path = changes.url;
            request.query = changes.query.into_iter().map(Field::from).collect();
            request.headers = changes.headers.into_iter().map(Field::from).collect();
            state.variables = output.variables;

            if changes.body_changed {
                body_changed = true;
                // A body that was not raw becomes raw JSON, as one added to
                // a request without a body.
                request.body = changes.body.map(|text| match request.body.take() {
                    Some(Body::Raw { language, .. }) => Body::Raw { language, text },
                    _ => Body::json(text),
                });
            } else {
                if let Some(part) = changes.parts.iter().flatten().find(|part| {
                    part.file && !attached.contains(&part.value)
                }) {
                    let message = format!(
                        "Scripts cannot attach files. Choose the file for \"{}\" in the request's body.",
                        part.name
                    );
                    report.error = Some(message.clone());
                    return Err(after_earlier_scripts(
                        reports,
                        ExecutionError::Script {
                            message,
                            report: Box::new(report),
                        },
                    ));
                }

                match (&mut request.body, changes.fields, changes.parts) {
                    (Some(Body::UrlEncoded { fields }), Some(changed), _) => *fields = changed,
                    (Some(Body::Multipart { parts }), _, Some(changed)) => *parts = changed,
                    _ => {}
                }
            }
            reports.push(report);
        }

        if matches!(request.method, Method::Get | Method::Head) {
            request.body = None;
        }

        let values = state.variables.visible();
        let resolved = resolve_request(
            &values,
            environment_error.as_deref(),
            request,
            scripted,
            body_changed,
            &mut state.variables.generated,
        );

        match resolved {
            Ok(request) => {
                if let Some(locals) = &state.locals {
                    locals.set(state.variables.values.clone());
                }
                Ok((request.prepare_for_send(), state, reports))
            }
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
            let mut input = input(&request, &state, "test");
            let Response::Http(response) = &mut execution.response;
            input["response"] = json!({
                "code": response.status.as_u16(),
                "status": response.status.canonical_reason().unwrap_or(""),
                "responseTime": execution.elapsed.as_secs_f64() * 1000.,
                "url": state.response_url,
                "headers": response.headers.iter().map(|(key, value)| {
                    (key.as_str(), String::from_utf8_lossy(value.as_bytes()).into_owned())
                }).collect::<Vec<_>>(),
            });
            let (output, mut report) = run::<serde::de::IgnoredAny>(
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
                && let Some(mut output) = output
            {
                report.next_request = output
                    .next_request
                    .take()
                    .map(NextRequestOutput::into_next_request);
                if let Some(session) = &state.session
                    && let Err(message) = session.apply(&output.changes)
                {
                    report.error = Some(message.into());
                } else {
                    state.variables = output.variables;
                }
            }
            execution.scripts.push(report);
        }
        if let Some(locals) = &state.locals {
            locals.set(state.variables.values.clone());
        }
        execution
    })
    .await
}

/// `event` names the phase as `pm.info.eventName` does.
fn input(request: &HttpRequest, state: &ScriptState, event: &str) -> serde_json::Value {
    let variables = &state.variables;
    // Raw text is read through the body reader instead.
    let body = match &request.body {
        None | Some(Body::Raw { .. }) => json!({ "mode": "raw" }),
        Some(Body::UrlEncoded { fields }) => json!({ "mode": "urlencoded", "fields": fields }),
        Some(Body::Multipart { parts }) => json!({ "mode": "formdata", "parts": parts }),
        Some(Body::Binary { file }) => json!({ "mode": "file", "file": file }),
    };

    json!({
        "method": request.method.as_str(),
        "url": request.path,
        // Where the request goes, which decides the jar's cookies for it.
        "sentUrl": sent_url(
            &request.path,
            &request.path_variables,
            &variables.visible(),
            &variables.generated,
        ),
        "query": Field::enabled(&request.query).collect::<Vec<_>>(),
        "headers": Field::enabled(&request.headers).collect::<Vec<_>>(),
        "body": body,
        "variables": variables,
        "dataText": variables.data_texts(),
        "info": state.info.input(event),
    })
}
