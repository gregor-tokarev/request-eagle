use std::{
    io::Read,
    time::{Duration, Instant},
};

use flate2::read::GzDecoder;
use request::ScriptPhase;
use rquickjs::{Context, Function, Runtime};

const MEMORY_LIMIT: usize = 128 * 1024 * 1024;
const INITIALIZATION_LIMIT: Duration = Duration::from_secs(15);
const QUERY_LIMIT: Duration = Duration::from_secs(2);
const RESPONSE_LIMIT: usize = 1024 * 1024;

pub(super) struct Compiler {
    context: Context,
    runtime: Runtime,
}

fn decompress(bytes: &[u8]) -> Result<String, String> {
    let mut source = String::new();

    GzDecoder::new(bytes)
        .take(12 * 1024 * 1024)
        .read_to_string(&mut source)
        .map_err(|error| format!("Unable to load embedded TypeScript: {error}"))?;

    Ok(source)
}

impl Compiler {
    pub fn new() -> Result<Self, String> {
        let started = Instant::now();
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(8 * 1024 * 1024);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            started.elapsed() >= INITIALIZATION_LIMIT
        })));
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;

        context.with(|cx| {
            let load = || -> rquickjs::Result<()> {
                let compiler = decompress(include_bytes!("assets/typescript-5.9.3.js.gz"))
                    .map_err(|error| rquickjs::Exception::throw_message(&cx, &error))?;

                // Only bundled compiler/host code is evaluated. Editor source is
                // passed to ScriptSnapshot as data and is never executed.
                cx.eval::<(), _>(compiler)?;
                let setup: Function = cx.eval(include_str!("host.js"))?;
                let libraries = decompress(include_bytes!("assets/lib.es2023.json.gz"))
                    .map_err(|error| rquickjs::Exception::throw_message(&cx, &error))?;
                let query: Function = setup.call((libraries, include_str!("pm.d.ts")))?;

                // Pay the first type-check/library binding cost while warming up.
                query.call::<_, String>(("pm.", 3, "pre", "completions"))?;
                cx.globals().set("requestEagleLanguageQuery", query)?;

                Ok(())
            };

            load().map_err(|error| rquickjs::CaughtError::from_error(&cx, error).to_string())
        })?;

        runtime.set_interrupt_handler(None);

        Ok(Self { context, runtime })
    }

    pub fn query(
        &mut self,
        source: &str,
        offset: usize,
        phase: ScriptPhase,
        kind: &str,
        cancelled: impl Fn() -> bool + 'static,
    ) -> Result<Option<String>, String> {
        let started = Instant::now();
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || started.elapsed() >= QUERY_LIMIT)));

        let result = self.context.with(|cx| {
            let execute = || -> rquickjs::Result<Option<String>> {
                let query: Function = cx.globals().get("requestEagleLanguageQuery")?;
                let phase = if phase == ScriptPhase::PreRequest {
                    "pre"
                } else {
                    "post"
                };

                let cancellation = Function::new(cx.clone(), cancelled)?;

                query.call((source, offset, phase, kind, cancellation))
            };

            execute().map_err(|error| {
                if started.elapsed() >= QUERY_LIMIT {
                    "Script language query exceeded its two-second limit".into()
                } else {
                    rquickjs::CaughtError::from_error(&cx, error).to_string()
                }
            })
        });

        self.runtime.set_interrupt_handler(None);
        let result = result?;

        if result
            .as_ref()
            .is_some_and(|result| result.len() > RESPONSE_LIMIT)
        {
            return Err("Script language response exceeds 1 MiB".into());
        }

        Ok(result)
    }
}
