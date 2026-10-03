use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use flow::{BlockKind, BlockRun, DisplayFormat, RunEvent, RunOptions, RunSummary, SavedRequest};
use futures::StreamExt as _;
use gpui_kit::*;
use preferences::Preferences;
use request::RequestExecutor;
use serde_json::Value;

use super::{FlowEditor, preview};
use crate::{cookies::Cookies, variables::VariableScope};

/// The most entries the run log keeps. A long loop logs much more; the
/// latest entries are the ones worth reading.
const LOG_LIMIT: usize = 2000;

/// About how many bytes of earlier runs' data the run log keeps for the
/// inspector. Older entries keep only their line of text.
const RUN_DATA_LIMIT: usize = 64 * 1024 * 1024;

/// How often a running flow redraws, at most.
const REDRAW_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Default)]
pub(super) struct RunState {
    pub task: Option<Task<()>>,
    pub started: Option<Instant>,
    pub blocks: HashMap<String, BlockStatus>,
    pub log: Vec<LogEntry>,
    /// Entries removed from the start of the log to keep it within its limit.
    pub dropped: usize,
    /// About how many bytes of run data the log's entries hold.
    pub retained: usize,
    pub summary: Option<RunSummary>,
    /// How many times an HTTP Request block's request failed, sending from
    /// its Fail output.
    pub failed_requests: usize,
    /// Why the flow could not run.
    pub error: Option<SharedString>,
}

#[derive(Default)]
pub(super) struct BlockStatus {
    pub running: bool,
    pub runs: usize,
    pub last: Option<Arc<BlockRun>>,
    /// What each output sent last in the run, whichever run of the block
    /// sent it, such as a loop's Then in an earlier iteration.
    pub outputs: HashMap<String, Arc<Value>>,
    /// A Display block's latest data, ready to draw, and the format it is
    /// drawn in.
    pub display: Option<(DisplayFormat, preview::Display)>,
}

impl BlockStatus {
    pub fn failed(&self) -> bool {
        self.last.as_ref().is_some_and(|run| run.error.is_some())
    }

    /// Whether the last run failed or sent from a Fail output: an HTTP
    /// Request's request or a Validate block's check failed.
    pub fn troubled(&self) -> bool {
        self.failed()
            || self
                .last
                .as_ref()
                .is_some_and(|run| run.outputs.iter().any(|(name, _)| name == "fail"))
    }

    /// The HTTP status of the response an HTTP Request block's last run
    /// received.
    pub fn http_status(&self) -> Option<u64> {
        self.last
            .as_ref()?
            .outputs
            .first()?
            .1
            .pointer("/http/status")?
            .as_u64()
    }

    /// Whether this output sent data in the run.
    pub fn sent(&self, output: &str) -> bool {
        self.outputs.contains_key(output)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LogKind {
    Ran,
    Notice,
    Failed,
    Logged,
}

pub(super) struct LogEntry {
    pub at: Duration,
    pub block: String,
    pub kind: LogKind,
    pub text: SharedString,
    /// Which run of its block this is, from 1, so a loop's runs can be told
    /// apart.
    pub index: usize,
    /// The run the entry tells of, which the inspector shows when the entry
    /// is chosen.
    pub run: RunData,
}

/// What a log entry keeps of the run it tells of.
pub(super) enum RunData {
    /// It tells of no block's run, such as the end of the flow's run.
    None,
    /// The run, and about how many bytes its data takes.
    Kept(Arc<BlockRun>, usize),
    /// The run's data was let go to keep memory in check. The run finished
    /// this long into the flow's run, which tells it apart from later runs
    /// of a block with the same ID.
    Released(Duration),
}

impl RunState {
    pub fn running(&self) -> bool {
        self.task.is_some()
    }
}

impl FlowEditor {
    /// Run the flow as it is in the editor, saved or not.
    pub fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run.running() {
            return;
        }

        if let Err(error) = self.check() {
            self.run.error = Some(error.into());
            self.log_open = true;
            cx.notify();
            return;
        }
        let options = match self.run_options(cx) {
            Ok(options) => options,
            Err(error) => {
                self.run.error = Some(error.into());
                self.log_open = true;
                cx.notify();
                return;
            }
        };

        let flow = self.flow.clone();
        let (sender, mut events) = futures::channel::mpsc::unbounded();
        let run = cx.background_executor().spawn(async move {
            flow::run(flow, options, move |event| {
                // Sizing a large response takes a while, so it happens here
                // rather than where the interface draws.
                let size = match &event {
                    RunEvent::Finished(run) => run_size(run),
                    _ => 0,
                };
                let _ = sender.unbounded_send((event, size));
            })
            .await
        });

        self.run = RunState {
            started: Some(Instant::now()),
            ..RunState::default()
        };
        self.log_open = true;
        self.run.task = Some(cx.spawn_in(window, async move |this, cx| {
            // Draw a batch of events at a time, so a fast flow cannot redraw
            // for every block it runs.
            while let Some(event) = events.next().await {
                let mut batch = vec![event];
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }

                if this.update(cx, |this, cx| this.receive(batch, cx)).is_err() {
                    return;
                }
                cx.background_executor().timer(REDRAW_INTERVAL).await;
            }

            let summary = run.await;
            let _ = this.update_in(cx, |this, window, cx| this.finish_run(summary, window, cx));
        }));
        cx.notify();
    }

    /// Stop the run. Requests already sent may still reach their servers.
    pub fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run.task.take().is_none() {
            return;
        }

        for status in self.run.blocks.values_mut() {
            status.running = false;
        }
        let at = self
            .run
            .started
            .map(|started| started.elapsed())
            .unwrap_or_default();
        self.log(at, String::new(), LogKind::Notice, "Run stopped".into());
        Cookies::changed(cx);
        self.refresh_inspector(window, cx);
        cx.notify();
    }

    /// Show each Display block's data in its current format, which undo,
    /// redo and the inspector change without running the flow again.
    pub(super) fn refresh_displays(&mut self) {
        for block in &self.flow.blocks {
            let BlockKind::Display { format } = block.kind else {
                continue;
            };
            if let Some(status) = self.run.blocks.get_mut(&block.id)
                && status
                    .display
                    .as_ref()
                    .is_some_and(|(shown, _)| *shown != format)
                && let Some((_, data)) = status.last.as_ref().and_then(|run| run.inputs.first())
            {
                status.display = Some((format, preview::Display::new(data, format)));
            }
        }
    }

    fn run_options(&self, cx: &App) -> Result<RunOptions, String> {
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        let mut executor = RequestExecutor::new(&preferences).map_err(|error| error.to_string())?;
        if let Some(jar) = crate::request_draft::active_jar(cx) {
            executor = executor.with_cookie_jar(jar);
        }

        let mut requests = HashMap::new();
        for block in &self.flow.blocks {
            let BlockKind::HttpRequest { request: id } = &block.kind else {
                continue;
            };
            if requests.contains_key(id) {
                continue;
            }
            let Some(saved) = self.requests.find(id, cx) else {
                continue;
            };

            // Requests resolve their variables as their own tabs do: from
            // their collection, the active environment and the session.
            let environment = saved.collection.join("environment.toml");
            let scope = VariableScope {
                path: Some(environment.clone()),
                session: self.sessions.for_path(Some(&environment)),
                environments: self.environments.clone(),
                names: None,
            };
            let (collection, environment, variables_error) = scope.file_values(cx);
            let settings = scope.collection_settings();
            let mut request = saved.request.clone();
            request.body = request
                .body
                .map(|body| body.resolved_from(&saved.collection));

            requests.insert(
                id.clone(),
                SavedRequest {
                    name: saved.name.to_string(),
                    request,
                    collection,
                    environment,
                    variables_error,
                    scripts: settings
                        .as_ref()
                        .map(|settings| settings.scripts.clone())
                        .map_err(Clone::clone),
                    auth: settings.map(|settings| settings.auth).unwrap_or_default(),
                    session: scope.session,
                },
            );
        }

        Ok(RunOptions {
            input: None,
            requests,
            executor,
        })
    }

    fn receive(&mut self, events: Vec<(RunEvent, usize)>, cx: &mut Context<Self>) {
        for (event, size) in events {
            match event {
                RunEvent::Started { block } => {
                    self.run.blocks.entry(block).or_default().running = true;
                }
                RunEvent::Log { block, value, at } => {
                    let text = preview::compact(&value, 400);
                    let index = self.run.blocks.get(&block).map_or(0, |status| status.runs) + 1;
                    self.log_run(at, block, LogKind::Logged, text, index, RunData::None);
                }
                RunEvent::Finished(run) => {
                    let run = Arc::new(run);
                    let block_kind = self.flow.block(&run.block).map(|block| &block.kind);
                    let request = matches!(block_kind, Some(BlockKind::HttpRequest { .. }));
                    let (kind, text) = match (&run.error, &run.notice) {
                        (Some(error), _) => (LogKind::Failed, error.clone().into()),
                        // A request that could not be sent goes out of Fail
                        // with a notice of why.
                        (None, Some(notice)) if request => (LogKind::Failed, notice.clone().into()),
                        (None, Some(notice)) => (LogKind::Notice, notice.clone().into()),
                        (None, None) if request => describe_response(&run.outputs),
                        // Blocks such as Output send nothing on; show what they received.
                        (None, None) if run.outputs.is_empty() && !run.inputs.is_empty() => {
                            (LogKind::Ran, describe("Received", &run.inputs))
                        }
                        (None, None) => (LogKind::Ran, describe("Sent", &run.outputs)),
                    };
                    // A Log block's entry already shows what it received.
                    let logged = matches!(block_kind, Some(BlockKind::Log))
                        && run.error.is_none()
                        && run.notice.is_none();
                    if request && run.outputs.iter().any(|(name, _)| name == "fail") {
                        self.run.failed_requests += 1;
                    }

                    let display = self
                        .flow
                        .block(&run.block)
                        .and_then(|block| match &block.kind {
                            BlockKind::Display { format } => run
                                .inputs
                                .first()
                                .map(|(_, value)| (*format, preview::Display::new(value, *format))),
                            _ => None,
                        });
                    let status = self.run.blocks.entry(run.block.clone()).or_default();
                    status.running = false;
                    status.runs += 1;
                    let index = status.runs;
                    if display.is_some() {
                        status.display = display;
                    }
                    status.last = Some(run.clone());
                    for (output, value) in &run.outputs {
                        status.outputs.insert(output.clone(), value.clone());
                    }

                    if logged {
                        // The entry the Log block wrote shows this run.
                        if let Some(entry) = self.run.log.iter_mut().rev().find(|entry| {
                            entry.block == run.block
                                && entry.kind == LogKind::Logged
                                && entry.index == index
                        }) && matches!(entry.run, RunData::None)
                        {
                            entry.run = RunData::Kept(run.clone(), size);
                            self.run.retained += size;
                            self.release_runs();
                        }
                    } else {
                        let data = RunData::Kept(run.clone(), size);
                        self.log_run(run.at, run.block.clone(), kind, text, index, data);
                    }
                }
            }
        }

        // Follow the newest entries while the run goes on.
        if let Some(last) = self.log_entries().len().checked_sub(1) {
            self.log_scroll.scroll_to_item(last, ScrollStrategy::Bottom);
        }
        cx.notify();
    }

    fn finish_run(&mut self, summary: RunSummary, window: &mut Window, cx: &mut Context<Self>) {
        self.run.task = None;
        for status in self.run.blocks.values_mut() {
            status.running = false;
        }

        let text = match &summary.stopped {
            Some(reason) => reason.clone(),
            None => format!(
                "Finished in {} with {} block run{}{}",
                format_duration(summary.elapsed),
                summary.block_runs,
                if summary.block_runs == 1 { "" } else { "s" },
                failures(summary.failures, self.run.failed_requests),
            ),
        };
        let kind =
            if summary.stopped.is_some() || summary.failures > 0 || self.run.failed_requests > 0 {
                LogKind::Failed
            } else {
                LogKind::Notice
            };
        self.log(summary.elapsed, String::new(), kind, text.into());
        self.run.summary = Some(summary);

        // Request scripts may have changed cookies and variables that other
        // tabs show.
        Cookies::changed(cx);
        window.refresh();
        self.refresh_inspector(window, cx);
        cx.notify();
    }

    fn log(&mut self, at: Duration, block: String, kind: LogKind, text: SharedString) {
        self.log_run(at, block, kind, text, 0, RunData::None);
    }

    fn log_run(
        &mut self,
        at: Duration,
        block: String,
        kind: LogKind,
        text: SharedString,
        index: usize,
        run: RunData,
    ) {
        if let RunData::Kept(_, size) = &run {
            self.run.retained += size;
        }
        self.run.log.push(LogEntry {
            at,
            block,
            kind,
            text,
            index,
            run,
        });

        if self.run.log.len() > LOG_LIMIT {
            let excess = self.run.log.len() - LOG_LIMIT;
            for entry in self.run.log.drain(..excess) {
                if let RunData::Kept(_, size) = entry.run {
                    self.run.retained -= size;
                }
            }
            self.run.dropped += excess;
        }
        self.release_runs();
    }

    /// Let earlier entries go of their run data while the log keeps too
    /// much. Each block's last run stays with its status.
    fn release_runs(&mut self) {
        let mut entries = self.run.log.iter_mut();
        while self.run.retained > RUN_DATA_LIMIT
            && let Some(entry) = entries.next()
        {
            if let RunData::Kept(run, size) = &entry.run {
                self.run.retained -= size;
                entry.run = RunData::Released(run.at);
            }
        }
    }
}

/// About how many bytes a run's inputs and outputs take.
fn run_size(run: &BlockRun) -> usize {
    fn size(value: &Value) -> usize {
        match value {
            Value::String(text) => text.len(),
            Value::Array(items) => items.iter().map(size).sum::<usize>() + items.len(),
            Value::Object(fields) => fields
                .iter()
                .map(|(name, value)| name.len() + size(value))
                .sum(),
            _ => 8,
        }
    }

    run.inputs
        .iter()
        .chain(&run.outputs)
        .map(|(name, value)| name.len() + size(value))
        .sum()
}

/// The failures of a run, such as `, 1 failed, 2 requests failed`, to
/// follow its summary.
pub(super) fn failures(blocks: usize, requests: usize) -> String {
    let mut text = String::new();
    match blocks {
        0 => {}
        1 => text.push_str(", 1 failed"),
        blocks => text.push_str(&format!(", {blocks} failed")),
    }
    match requests {
        0 => {}
        1 => text.push_str(", 1 request failed"),
        requests => text.push_str(&format!(", {requests} requests failed")),
    }
    text
}

/// What an HTTP Request block received, in one line: its status, the
/// output it went out of, and the response body. A failed request is a
/// failure of the run.
pub(super) fn describe_response(outputs: &[(String, Arc<Value>)]) -> (LogKind, SharedString) {
    let Some((output, value)) = outputs.first() else {
        return (LogKind::Ran, "Sent nothing".into());
    };
    let kind = if output == "fail" {
        LogKind::Failed
    } else {
        LogKind::Ran
    };
    let status = value
        .pointer("/http/status")
        .and_then(Value::as_u64)
        .map(|status| format!("{status} · "))
        .unwrap_or_default();
    let body = value.get("body").unwrap_or(value);

    (
        kind,
        format!("{status}{output}: {}", preview::compact(body, 300)).into(),
    )
}

/// What a block sent or received, in one line.
fn describe(verb: &str, values: &[(String, Arc<Value>)]) -> SharedString {
    match values {
        [] => format!("{verb} nothing").into(),
        [(name, value)] => format!("{name}: {}", preview::compact(value, 300)).into(),
        values => values
            .iter()
            .map(|(name, value)| format!("{name}: {}", preview::compact(value, 120)))
            .collect::<Vec<_>>()
            .join(" · ")
            .into(),
    }
}

pub(super) fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{} ms", duration.as_millis())
    } else {
        format!("{:.2} s", duration.as_secs_f64())
    }
}
