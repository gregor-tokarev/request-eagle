use std::{
    borrow::Cow,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use rquickjs::{
    Context, Exception, Function, Object, Promise, Runtime, Value,
    function::{Args, This},
};
use serde::Deserialize;

use super::{
    ScriptLog, ScriptPhase, ScriptReport, ScriptTest,
    network::{Network, NetworkOptions},
    variables::{Variables, dynamic_variable},
};
use crate::Method;

const TIME_LIMIT: Duration = Duration::from_secs(2);
const WALL_LIMIT: Duration = Duration::from_secs(30);
const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 500;

#[derive(Deserialize)]
pub(super) struct ScriptOutput {
    pub method: Method,
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_changed: bool,
    pub variables: Variables,
    pub environment_changes: BTreeMap<String, Option<String>>,
    pub skip_reason: Option<String>,
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

pub(super) fn run(
    source: &str,
    phase: ScriptPhase,
    input: serde_json::Value,
    request_body: &mut Option<Bytes>,
    mut response_body: Option<&mut Vec<u8>>,
    cancelled: Arc<AtomicBool>,
    network_options: Option<NetworkOptions>,
) -> (Option<ScriptOutput>, ScriptReport) {
    // Callbacks own the buffers while QuickJS runs. Restore them after the
    // runtime is dropped, including on script errors, without copying bytes.
    let bodies = Rc::new(Bodies {
        request: request_body.take(),
        response: response_body.as_mut().map(|body| std::mem::take(*body)),
    });
    let report = Arc::new(Mutex::new(ScriptReport {
        phase,
        collection: false,
        tests: Vec::new(),
        logs: Vec::new(),
        error: None,
    }));
    let started = Instant::now();
    let waiting = Arc::new(AtomicU64::new(0));
    let result = (|| -> Result<ScriptOutput, String> {
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(512 * 1024);
        let interrupted = cancelled.clone();
        let paused = waiting.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupted.load(Ordering::Relaxed)
                || started.elapsed() >= WALL_LIMIT
                || started
                    .elapsed()
                    .saturating_sub(Duration::from_nanos(paused.load(Ordering::Relaxed)))
                    >= TIME_LIMIT
        })));
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;

        // QuickJS reports each initially unhandled rejection once, and reports
        // it again if a handler is attached later. Counting avoids holding JS
        // values across phases or relying on reusable promise pointer identities.
        let rejected = Arc::new(AtomicUsize::new(0));
        let rejections = rejected.clone();
        runtime.set_host_promise_rejection_tracker(Some(Box::new(move |_, _, _, handled| {
            if handled {
                rejections.fetch_sub(1, Ordering::Relaxed);
            } else {
                rejections.fetch_add(1, Ordering::Relaxed);
            }
        })));

        let output = context.with(|cx| {
            let network = Network::default();
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
                let test =
                    Function::new(cx.clone(), move |name: String, error: Option<String>| {
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
                let send = network.binding(cx.clone(), network_options)?;
                let utilities = super::utilities::bindings(cx.clone())?;
                let setup: Function = cx.eval(include_str!("sandbox.js"))?;
                let mut args = Args::new(cx.clone(), 8);
                args.push_arg(input.to_string())?;
                args.push_arg(log)?;
                args.push_arg(test)?;
                args.push_arg(expect)?;
                args.push_arg(dynamic)?;
                args.push_arg(read_body)?;
                args.push_arg(send)?;
                args.push_arg(utilities)?;
                let state: Object = setup.call_arg(args)?;
                let export: Function = state.get("export")?;
                let skipped: Function = state.get("isSkipped")?;
                let execute: Function = cx.eval(format!("(async function () {{\n{source}\n}})"))?;
                let main: Promise = execute.call(())?;
                // Observe the root rejection here; unrelated rejected promises remain errors.
                main.catch()?
                    .call::<_, Value>((This(main.clone()), Function::new(cx.clone(), || {})?))?;

                loop {
                    if cancelled.load(Ordering::Relaxed)
                        || started.elapsed() >= WALL_LIMIT
                        || started
                            .elapsed()
                            .saturating_sub(Duration::from_nanos(waiting.load(Ordering::Relaxed)))
                            >= TIME_LIMIT
                    {
                        return Err(Exception::throw_message(&cx, "Script interrupted"));
                    }
                    if skipped.call::<_, bool>(())? {
                        break;
                    }
                    if cx.execute_pending_job() {
                        continue;
                    }
                    if let Some(Err(error)) = main.result::<Value>() {
                        return Err(error);
                    }
                    if network.is_empty() {
                        if main.result::<Value>().is_none() {
                            return Err(Exception::throw_message(
                                &cx,
                                "Script is awaiting a promise that cannot settle",
                            ));
                        }
                        if rejected.load(Ordering::Relaxed) > 0 {
                            return Err(Exception::throw_message(
                                &cx,
                                "Unhandled promise rejection in script",
                            ));
                        }
                        break;
                    }

                    let wait_started = Instant::now();
                    let completion = smol::block_on(smol::future::or(network.next(), async {
                        smol::Timer::after(Duration::from_millis(10)).await;
                        None
                    }));
                    waiting.fetch_add(wait_started.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    if let Some((resolve, reject, result)) = completion {
                        match result {
                            Ok(json) => resolve.call::<_, ()>((json,))?,
                            Err(message) => reject
                                .call::<_, ()>((Exception::from_message(cx.clone(), &message)?,))?,
                        }
                    }
                }

                export.call(())
            };

            let evaluated = evaluate();
            network.clear();
            let serialized = evaluated.map_err(|error| {
                if cancelled.load(Ordering::Relaxed) {
                    "Script cancelled".to_owned()
                } else if started.elapsed() >= WALL_LIMIT {
                    "Script exceeded the 30 second elapsed time limit".to_owned()
                } else if started
                    .elapsed()
                    .saturating_sub(Duration::from_nanos(waiting.load(Ordering::Relaxed)))
                    >= TIME_LIMIT
                {
                    "Script exceeded the 2 second time limit".to_owned()
                } else {
                    rquickjs::CaughtError::from_error(&cx, error).to_string()
                }
            })?;

            serde_json::from_str(&serialized)
                .map_err(|error| format!("Invalid script request data: {error}"))
        })?;

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
