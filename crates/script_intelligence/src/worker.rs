use std::{
    collections::VecDeque,
    mem,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use event_listener::Event;
use futures_channel::oneshot;
use lsp_types::{CompletionItem, Hover, SignatureHelp};
use request::ScriptPhase;

use super::compiler::Compiler;

const SOURCE_LIMIT: usize = 256 * 1024;
const QUEUE_LIMIT: usize = 16;
// Release the compiler after scripts have not been edited for this long.
const IDLE_LIMIT: Duration = Duration::from_secs(5 * 60);

struct Query {
    source: String,
    offset: usize,
    phase: ScriptPhase,
    kind: &'static str,
    cancelled: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<String, String>>,
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Pending {
    queries: VecDeque<Query>,
    warm_up: bool,
}

#[derive(Default)]
struct Queue {
    pending: Mutex<Pending>,
    available: Condvar,
    capacity: Event,
}

enum Work {
    Query(Query),
    WarmUp,
    Idle,
}

impl Queue {
    async fn push(&self, query: Query) {
        loop {
            // Register before checking capacity so a concurrent pop cannot
            // strand this request between the check and awaiting notification.
            let capacity = self.capacity.listen();

            {
                let mut pending = self.pending.lock().unwrap();
                pending.queries.retain(|query| {
                    !query.cancelled.load(Ordering::Relaxed) && !query.reply.is_canceled()
                });

                if pending.queries.len() < QUEUE_LIMIT {
                    pending.queries.push_back(query);
                    self.available.notify_one();
                    self.capacity.notify(QUEUE_LIMIT - pending.queries.len());

                    return;
                }
            }

            // Distinct live editors can fill the queue during cold startup.
            // Apply async backpressure rather than losing their final queries.
            capacity.await;
        }
    }

    fn warm_up(&self) {
        self.pending.lock().unwrap().warm_up = true;
        self.available.notify_one();
    }

    /// Wait for a query or warm-up request, or report that none arrived
    /// within `idle`.
    fn next(&self, idle: Duration) -> Work {
        let pending = self.pending.lock().unwrap();
        let (mut pending, _) = self
            .available
            .wait_timeout_while(pending, idle, |pending| {
                pending.queries.is_empty() && !pending.warm_up
            })
            .unwrap();

        if let Some(query) = pending.queries.pop_front() {
            self.capacity.notify(1);
            Work::Query(query)
        } else if mem::take(&mut pending.warm_up) {
            Work::WarmUp
        } else {
            Work::Idle
        }
    }
}

/// Return a released compiler's memory to the system. The allocator would
/// otherwise keep the freed pages for the process.
fn release_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only releases memory that is already free.
    unsafe {
        libc::malloc_trim(0);
    }

    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }

        // SAFETY: A null zone asks every malloc zone to release free pages.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}

fn worker() -> Result<&'static Arc<Queue>> {
    static WORKER: OnceLock<Result<Arc<Queue>, String>> = OnceLock::new();

    WORKER
        .get_or_init(|| {
            let queue = Arc::new(Queue::default());
            let worker_queue = queue.clone();

            thread::Builder::new()
                .name("script-typescript".into())
                .stack_size(16 * 1024 * 1024)
                .spawn(move || {
                    // The compiler and static libraries load off the UI thread
                    // on warm-up or the first query, and unload when idle.
                    let mut compiler = None;
                    // Failed queries drop their compiler before the worker
                    // goes idle, so track memory to release separately.
                    let mut used_since_release = false;

                    loop {
                        let query = match worker_queue.next(IDLE_LIMIT) {
                            Work::Query(query) => query,
                            Work::WarmUp => {
                                if compiler.is_none() {
                                    // The next query retries and reports a failure.
                                    used_since_release = true;
                                    compiler = Compiler::new().ok();
                                }
                                continue;
                            }
                            Work::Idle => {
                                compiler = None;
                                if mem::take(&mut used_since_release) {
                                    release_freed_memory();
                                }
                                continue;
                            }
                        };

                        if query.cancelled.load(Ordering::Relaxed) || query.reply.is_canceled() {
                            continue;
                        }

                        used_since_release = true;
                        let loaded = compiler.take().map_or_else(Compiler::new, Ok);
                        let result = loaded.and_then(|mut loaded| {
                            let cancelled = query.cancelled.clone();
                            let result = loaded.query(
                                &query.source,
                                query.offset,
                                query.phase,
                                query.kind,
                                move || cancelled.load(Ordering::Relaxed),
                            );

                            // Hard interrupts/errors can leave a partially
                            // updated TS program, so the next query loads a
                            // fresh one. Dropping this one first means two
                            // never coexist. Cooperative TS cancellation
                            // returns Ok(None) and safely keeps the warm compiler.
                            if result.is_ok() {
                                compiler = Some(loaded);
                            }

                            result
                        });
                        let _ = query.reply.send(result.and_then(|result| {
                            result.ok_or_else(|| "Script language query was cancelled".into())
                        }));
                    }
                })
                .map_err(|error| format!("Unable to start script language service: {error}"))?;

            Ok(queue)
        })
        .as_ref()
        .map_err(|error| anyhow!(error.clone()))
}

/// Load the bundled compiler/libraries while a script editor becomes visible
/// or focused, unless they are already loaded.
pub fn warm_up() {
    if let Ok(queue) = worker() {
        queue.warm_up();
    }
}

async fn query(
    source: String,
    offset: usize,
    phase: ScriptPhase,
    kind: &'static str,
) -> Result<String> {
    if source.len() > SOURCE_LIMIT {
        bail!("Script intelligence supports sources up to 256 KiB");
    }

    if !source.is_char_boundary(offset) {
        bail!("Script intelligence cursor is not a UTF-8 boundary");
    }

    let offset = source[..offset].encode_utf16().count();
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancellation = CancelOnDrop(cancelled.clone());
    let (reply, response) = oneshot::channel();

    worker()?
        .push(Query {
            source,
            offset,
            phase,
            kind,
            cancelled,
            reply,
        })
        .await;

    response
        .await
        .context("Script language service stopped")?
        .map_err(anyhow::Error::msg)
}

pub async fn completions(
    source: String,
    offset: usize,
    phase: ScriptPhase,
) -> Result<Vec<CompletionItem>> {
    let result = query(source, offset, phase, "completions").await?;

    serde_json::from_str(&result).context("Invalid TypeScript completion response")
}

pub async fn signature_help(
    source: String,
    offset: usize,
    phase: ScriptPhase,
) -> Result<Option<SignatureHelp>> {
    let result = query(source, offset, phase, "signature").await?;

    serde_json::from_str(&result).context("Invalid TypeScript signature response")
}

pub async fn hover(source: String, offset: usize, phase: ScriptPhase) -> Result<Option<Hover>> {
    let result = query(source, offset, phase, "hover").await?;

    serde_json::from_str(&result).context("Invalid TypeScript hover response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next_query(queue: &Queue) -> Query {
        let Work::Query(query) = queue.next(Duration::ZERO) else {
            panic!("expected a query");
        };

        query
    }

    fn query(source: &str) -> (Query, oneshot::Receiver<Result<String, String>>) {
        let (reply, receiver) = oneshot::channel();
        let query = Query {
            source: source.into(),
            offset: 0,
            phase: ScriptPhase::PreRequest,
            kind: "completions",
            cancelled: Arc::new(AtomicBool::new(false)),
            reply,
        };

        (query, receiver)
    }

    #[test]
    fn cold_start_queue_keeps_latest_query_after_cancelled_typeahead() {
        let queue = Queue::default();
        let mut receivers = Vec::new();

        for _ in 0..QUEUE_LIMIT {
            let (query, receiver) = query("older input");
            smol::block_on(queue.push(query));
            receivers.push(receiver);
        }

        drop(receivers);
        let (latest, _receiver) = query("latest input");
        smol::block_on(queue.push(latest));

        assert_eq!(next_query(&queue).source, "latest input");
        assert!(queue.pending.lock().unwrap().queries.is_empty());
    }

    #[test]
    fn full_live_queue_preserves_the_waiting_query_until_capacity_is_available() {
        let queue = Queue::default();
        let mut receivers = Vec::new();

        for _ in 0..QUEUE_LIMIT {
            let (query, receiver) = query("other editor");
            smol::block_on(queue.push(query));
            receivers.push(receiver);
        }

        let (latest, _receiver) = query("final input");
        let mut enqueue = Box::pin(queue.push(latest));
        assert!(smol::block_on(smol::future::poll_once(&mut enqueue)).is_none());
        assert_eq!(queue.pending.lock().unwrap().queries.len(), QUEUE_LIMIT);

        // The waiting future resumes after a pop and retains its actual query.
        assert_eq!(next_query(&queue).source, "other editor");
        smol::block_on(enqueue);

        for _ in 1..QUEUE_LIMIT {
            assert_eq!(next_query(&queue).source, "other editor");
        }

        assert_eq!(next_query(&queue).source, "final input");
        assert!(queue.pending.lock().unwrap().queries.is_empty());
    }

    #[test]
    fn idle_worker_is_told_to_unload_until_a_warm_up_or_query_arrives() {
        let queue = Queue::default();
        assert!(matches!(queue.next(Duration::ZERO), Work::Idle));

        queue.warm_up();
        assert!(matches!(queue.next(Duration::ZERO), Work::WarmUp));
        assert!(matches!(queue.next(Duration::ZERO), Work::Idle));

        let (latest, _receiver) = query("latest input");
        smol::block_on(queue.push(latest));
        assert_eq!(next_query(&queue).source, "latest input");
        assert!(matches!(queue.next(Duration::ZERO), Work::Idle));
    }
}
