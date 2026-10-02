use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use flow::{BlockKind, BlockRun, RunEvent, RunOptions, RunSummary, SavedRequest};
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
    pub summary: Option<RunSummary>,
    /// Why the flow could not run.
    pub error: Option<SharedString>,
}

#[derive(Default)]
pub(super) struct BlockStatus {
    pub running: bool,
    pub runs: usize,
    pub last: Option<BlockRun>,
    /// A Display block's latest data, ready to draw.
    pub display: Option<preview::Display>,
}

impl BlockStatus {
    pub fn failed(&self) -> bool {
        self.last.as_ref().is_some_and(|run| run.error.is_some())
    }

    /// Whether the last run sent data from this output.
    pub fn sent(&self, output: &str) -> bool {
        self.last
            .as_ref()
            .is_some_and(|run| run.outputs.iter().any(|(name, _)| name == output))
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
                let _ = sender.unbounded_send(event);
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
                    scripts: scope.collection_scripts(),
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

    fn receive(&mut self, events: Vec<RunEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                RunEvent::Started { block } => {
                    self.run.blocks.entry(block).or_default().running = true;
                }
                RunEvent::Log { block, value, at } => {
                    let text = preview::compact(&value, 400);
                    self.log(at, block, LogKind::Logged, text);
                }
                RunEvent::Finished(run) => {
                    let (kind, text) = match (&run.error, &run.notice) {
                        (Some(error), _) => (LogKind::Failed, error.clone().into()),
                        (None, Some(notice)) => (LogKind::Notice, notice.clone().into()),
                        // Blocks such as Output send nothing on; show what they received.
                        (None, None) if run.outputs.is_empty() && !run.inputs.is_empty() => {
                            (LogKind::Ran, describe("Received", &run.inputs))
                        }
                        (None, None) => (LogKind::Ran, describe("Sent", &run.outputs)),
                    };
                    self.log(run.at, run.block.clone(), kind, text);

                    let display = self
                        .flow
                        .block(&run.block)
                        .and_then(|block| match &block.kind {
                            BlockKind::Display { format } => run
                                .inputs
                                .first()
                                .map(|(_, value)| preview::Display::new(value, *format)),
                            _ => None,
                        });
                    let status = self.run.blocks.entry(run.block.clone()).or_default();
                    status.running = false;
                    status.runs += 1;
                    if display.is_some() {
                        status.display = display;
                    }
                    status.last = Some(run);
                }
            }
        }

        // Follow the newest entries while the run goes on.
        if let Some(last) = self.run.log.len().checked_sub(1) {
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
                match summary.failures {
                    0 => String::new(),
                    1 => ", 1 failed".to_owned(),
                    failures => format!(", {failures} failed"),
                }
            ),
        };
        let kind = if summary.stopped.is_some() || summary.failures > 0 {
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
        self.run.log.push(LogEntry {
            at,
            block,
            kind,
            text,
        });

        if self.run.log.len() > LOG_LIMIT {
            let excess = self.run.log.len() - LOG_LIMIT;
            self.run.log.drain(..excess);
            self.run.dropped += excess;
        }
    }
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
