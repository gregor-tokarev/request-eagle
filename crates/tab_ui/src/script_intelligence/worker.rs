use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context as _, Result, anyhow, bail};
use event_listener::Event;
use futures_channel::oneshot;
use lsp_types::{CompletionItem, Hover, SignatureHelp};
use request::ScriptPhase;

use super::compiler::Compiler;

const SOURCE_LIMIT: usize = 256 * 1024;
const QUEUE_LIMIT: usize = 16;

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
struct Queue {
    pending: Mutex<VecDeque<Query>>,
    available: Condvar,
    capacity: Event,
}

impl Queue {
    async fn push(&self, query: Query) {
        loop {
            // Register before checking capacity so a concurrent pop cannot
            // strand this request between the check and awaiting notification.
            let capacity = self.capacity.listen();

            {
                let mut pending = self.pending.lock().unwrap();
                pending.retain(|query| {
                    !query.cancelled.load(Ordering::Relaxed) && !query.reply.is_canceled()
                });

                if pending.len() < QUEUE_LIMIT {
                    pending.push_back(query);
                    self.available.notify_one();
                    self.capacity.notify(QUEUE_LIMIT - pending.len());

                    return;
                }
            }

            // Distinct live editors can fill the queue during cold startup.
            // Apply async backpressure rather than losing their final queries.
            capacity.await;
        }
    }

    fn next(&self) -> Query {
        let mut pending = self.pending.lock().unwrap();

        loop {
            if let Some(query) = pending.pop_front() {
                self.capacity.notify(1);
                return query;
            }

            pending = self.available.wait(pending).unwrap();
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
                    // Compiler and static libraries initialize once, off the UI
                    // thread, before processing editor requests.
                    let mut compiler = Compiler::new();

                    loop {
                        let query = worker_queue.next();
                        if query.cancelled.load(Ordering::Relaxed) || query.reply.is_canceled() {
                            continue;
                        }

                        let result =
                            compiler
                                .as_mut()
                                .map_err(|error| error.clone())
                                .and_then(|compiler| {
                                    let cancelled = query.cancelled.clone();
                                    compiler.query(
                                        &query.source,
                                        query.offset,
                                        query.phase,
                                        query.kind,
                                        move || cancelled.load(Ordering::Relaxed),
                                    )
                                });
                        let failed = result.is_err();
                        let _ = query.reply.send(result.and_then(|result| {
                            result.ok_or_else(|| "Script language query was cancelled".into())
                        }));

                        if failed {
                            // Hard interrupts/errors can leave a partially
                            // updated TS program. Cooperative TS cancellation
                            // returns Ok(None) and safely keeps the warm compiler.
                            compiler = Compiler::new();
                        }
                    }
                })
                .map_err(|error| format!("Unable to start script language service: {error}"))?;

            Ok(queue)
        })
        .as_ref()
        .map_err(|error| anyhow!(error.clone()))
}

/// Start parsing the bundled compiler/libraries while the editor becomes visible.
pub(crate) fn warm_up() {
    let _ = worker();
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

pub(crate) async fn completions(
    source: String,
    offset: usize,
    phase: ScriptPhase,
) -> Result<Vec<CompletionItem>> {
    let result = query(source, offset, phase, "completions").await?;

    serde_json::from_str(&result).context("Invalid TypeScript completion response")
}

pub(crate) async fn signature_help(
    source: String,
    offset: usize,
    phase: ScriptPhase,
) -> Result<Option<SignatureHelp>> {
    let result = query(source, offset, phase, "signature").await?;

    serde_json::from_str(&result).context("Invalid TypeScript signature response")
}

pub(crate) async fn hover(
    source: String,
    offset: usize,
    phase: ScriptPhase,
) -> Result<Option<Hover>> {
    let result = query(source, offset, phase, "hover").await?;

    serde_json::from_str(&result).context("Invalid TypeScript hover response")
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(queue.next().source, "latest input");
        assert!(queue.pending.lock().unwrap().is_empty());
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
        assert_eq!(queue.pending.lock().unwrap().len(), QUEUE_LIMIT);

        // The waiting future resumes after a pop and retains its actual query.
        assert_eq!(queue.next().source, "other editor");
        smol::block_on(enqueue);

        for _ in 1..QUEUE_LIMIT {
            assert_eq!(queue.next().source, "other editor");
        }

        assert_eq!(queue.next().source, "final input");
        assert!(queue.pending.lock().unwrap().is_empty());
    }
}
