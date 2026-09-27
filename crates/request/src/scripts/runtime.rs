use std::{
    borrow::Cow,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use rquickjs::{Context, Function, Runtime, Value};
use serde::Deserialize;
use serde_json::json;

use super::{
    ScriptLog, ScriptPhase, ScriptReport, ScriptTest,
    variables::{Variables, dynamic_variable, expand_request, has_dynamic_placeholders},
};
use crate::{Execution, ExecutionError, HttpRequest, Method, Response};

const TIME_LIMIT: Duration = Duration::from_secs(2);
const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 500;

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

#[derive(Deserialize)]
struct ScriptOutput {
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<String>,
    body_changed: bool,
    variables: Variables,
}

struct Bodies {
    request: Option<Bytes>,
    response: Option<Vec<u8>>,
}

fn body_reader<'js>(cx: rquickjs::Ctx<'js>, bodies: Rc<Bodies>) -> rquickjs::Result<Function<'js>> {
    Function::new(cx, move |cx: rquickjs::Ctx<'js>, response: bool| {
        let bytes = if response {
            bodies.response.as_deref()
        } else {
            bodies.request.as_deref()
        };
        match bytes {
            Some(bytes) => {
                let text = body_text(bytes).map_err(|()| {
                    rquickjs::Exception::throw_range(
                        &cx,
                        "Script body exceeds the 32 MiB decoding limit",
                    )
                })?;
                rquickjs::String::from_str(cx, &text).map(rquickjs::String::into_value)
            }
            None => Ok(Value::new_null(cx)),
        }
    })
}

fn body_text(bytes: &[u8]) -> Result<Cow<'_, str>, ()> {
    if bytes.len() > MEMORY_LIMIT {
        return Err(());
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(Cow::Borrowed(text));
    }

    // Count replacement characters before allocating: lossy UTF-8 decoding
    // can triple the input size outside QuickJS's allocator.
    let length = bytes.utf8_chunks().try_fold(0usize, |length, chunk| {
        let length = length + chunk.valid().len() + usize::from(!chunk.invalid().is_empty()) * 3;
        (length <= MEMORY_LIMIT).then_some(length).ok_or(())
    })?;
    let mut text = String::with_capacity(length);
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push('\u{fffd}');
        }
    }
    Ok(Cow::Owned(text))
}

#[cfg(test)]
pub(super) async fn pre_request(
    request: HttpRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<(HttpRequest, Variables, Vec<ScriptReport>), ExecutionError> {
    pre_request_with_variables(request, cancelled, None).await
}

pub(crate) async fn pre_request_with_variables(
    mut request: HttpRequest,
    cancelled: Arc<AtomicBool>,
    mut context: Option<crate::RequestVariables>,
) -> Result<(HttpRequest, Variables, Vec<ScriptReport>), ExecutionError> {
    let has_script = !request.scripts.pre_request.trim().is_empty();
    if context.is_none() && !has_script && !has_dynamic_placeholders(&request) {
        return Ok((request, Variables::new(), Vec::new()));
    }

    smol::unblock(move || {
        let mut report = ScriptReport {
            phase: ScriptPhase::PreRequest,
            tests: Vec::new(),
            logs: Vec::new(),
            error: None,
        };
        let mut variables: Variables = context
            .as_mut()
            .map(|context| {
                std::mem::take(&mut context.values.environment)
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();

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
                cancelled,
            );
            request.body = body.map(Vec::from);
            report = script_report;

            if let Some(message) = &report.error {
                return Err(ExecutionError::Script {
                    message: message.clone(),
                    report: Box::new(report),
                });
            }

            let output = output.expect("successful script output");
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
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            context.resolve_owned(request, body_changed)
        } else {
            expand_request(&mut request, &variables, body_changed).map(|()| request)
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

pub(crate) async fn post_response(
    request: HttpRequest,
    mut request_body: Option<Bytes>,
    variables: Variables,
    mut execution: Execution,
    cancelled: Arc<AtomicBool>,
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
        let (_, report) = run(
            &request.scripts.post_response,
            ScriptPhase::PostResponse,
            input,
            &mut request_body,
            Some(&mut response.body),
            cancelled,
        );
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

fn run(
    source: &str,
    phase: ScriptPhase,
    input: serde_json::Value,
    request_body: &mut Option<Bytes>,
    mut response_body: Option<&mut Vec<u8>>,
    cancelled: Arc<AtomicBool>,
) -> (Option<ScriptOutput>, ScriptReport) {
    // Callbacks own the buffers while QuickJS runs. Restore them after the
    // runtime is dropped, including on script errors, without copying bytes.
    let bodies = Rc::new(Bodies {
        request: request_body.take(),
        response: response_body.as_mut().map(|body| std::mem::take(*body)),
    });
    let report = Arc::new(Mutex::new(ScriptReport {
        phase,
        tests: Vec::new(),
        logs: Vec::new(),
        error: None,
    }));
    let started = Instant::now();
    let result = (|| -> Result<ScriptOutput, String> {
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(512 * 1024);
        let interrupted = cancelled.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupted.load(Ordering::Relaxed) || started.elapsed() >= TIME_LIMIT
        })));
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;

        let rejected = Arc::new(AtomicBool::new(false));
        let rejection_flag = rejected.clone();
        runtime.set_host_promise_rejection_tracker(Some(Box::new(move |_, _, _, _| {
            rejection_flag.store(true, Ordering::Relaxed);
        })));

        let output = context.with(|cx| {
            let evaluate = || -> rquickjs::Result<String> {
                let logs = report.clone();
                let log = Function::new(cx.clone(), move |level: String, message: String| {
                    let mut report = logs.lock().unwrap();
                    if report.logs.len() < OUTPUT_LIMIT {
                        report.logs.push(ScriptLog {
                            level,
                            message: message.chars().take(4096).collect(),
                        });
                    }
                })?;
                let tests = report.clone();
                let test = Function::new(cx.clone(), move |name: String, error: Option<String>| {
                    let mut report = tests.lock().unwrap();
                    if report.tests.len() >= OUTPUT_LIMIT {
                        report.error = Some("Script exceeded the 500 test limit".into());
                    } else {
                        report.tests.push(ScriptTest {
                            name: name.chars().take(4096).collect(),
                            error: error.map(|s| s.chars().take(4096).collect()),
                        });
                    }
                })?;
                let expect: Function = cx.eval(include_str!("assertions.js"))?;
                let dynamic = Function::new(cx.clone(), |name: String| dynamic_variable(&name))?;
                let read_body = body_reader(cx.clone(), bodies.clone())?;
                let setup: Function = cx.eval(include_str!("sandbox.js"))?;
                let export: Function = setup.call((input.to_string(), log, test, expect, dynamic, read_body))?;
                let value: Value = cx.eval(source)?;

                if value.is_promise() {
                    return Err(rquickjs::Exception::throw_type(&cx, "Scripts must be synchronous; promises and async functions are not supported"));
                }

                export.call(())
            };

            let serialized = evaluate().map_err(|error| {
                if cancelled.load(Ordering::Relaxed) {
                    "Script cancelled".to_owned()
                } else if started.elapsed() >= TIME_LIMIT {
                    "Script exceeded the 2 second time limit".to_owned()
                } else {
                    rquickjs::CaughtError::from_error(&cx, error).to_string()
                }
            })?;

            serde_json::from_str(&serialized).map_err(|error| format!("Invalid script request data: {error}"))
        })?;

        if runtime.is_job_pending() || rejected.load(Ordering::Relaxed) {
            return Err(
                "Scripts must be synchronous; promises and async functions are not supported"
                    .into(),
            );
        }

        Ok(output)
    })();

    let bodies = Rc::try_unwrap(bodies)
        .ok()
        .expect("runtime releases body callbacks");
    *request_body = bodies.request;
    if let Some(response_body) = response_body {
        *response_body = bodies.response.unwrap();
    }
    let mut report = report.lock().unwrap().clone();

    match result {
        Ok(output) => (Some(output), report),
        Err(error) => {
            report.error = Some(error.chars().take(4096).collect());
            (None, report)
        }
    }
}
