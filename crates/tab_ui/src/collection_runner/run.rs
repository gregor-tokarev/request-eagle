use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui_kit::SharedString;
use request::{
    Execution, ExecutionError, ExecutionFailure, HttpRequest, Method, NextRequest, Response,
    ScriptReport,
};

/// A saved HTTP request that a run can send.
#[derive(Clone, Debug)]
pub(crate) struct RunRequest {
    /// The file the request is saved in.
    pub path: PathBuf,
    pub id: SharedString,
    pub name: SharedString,
    /// The folders between the collection and the request.
    pub folders: Vec<SharedString>,
    pub request: HttpRequest,
}

/// An iteration, counting from 0, and a request's place in the run order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Position {
    pub iteration: usize,
    pub index: usize,
}

/// Which request a run sends next. Requests go in the run order, unless a
/// script chooses another with `pm.execution.setNextRequest`.
pub(crate) struct Cursor {
    next: Option<Position>,
    iterations: usize,
}

impl Cursor {
    pub fn new(requests: usize, iterations: usize) -> Self {
        Self {
            next: (requests > 0 && iterations > 0).then_some(Position {
                iteration: 0,
                index: 0,
            }),
            iterations,
        }
    }

    /// None once the run is complete.
    pub fn next(&self) -> Option<Position> {
        self.next
    }

    /// Move past the request at `next`, following what its scripts chose.
    /// Returns why the iteration ended early when the chosen request is not
    /// in the run.
    pub fn advance(
        &mut self,
        choice: Option<&NextRequest>,
        requests: &[RunRequest],
    ) -> Option<String> {
        let current = self.next?;
        let mut warning = None;
        let index = match choice {
            None => Some(current.index + 1).filter(|&index| index < requests.len()),
            Some(NextRequest::Stop) => None,
            // Like Postman, an ID or else the first request with the name.
            Some(NextRequest::Request(target)) => {
                let found = requests
                    .iter()
                    .position(|request| request.id.as_ref() == target)
                    .or_else(|| {
                        requests
                            .iter()
                            .position(|request| request.name.as_ref() == target)
                    });

                if found.is_none() {
                    warning = Some(format!(
                        "pm.execution.setNextRequest(\"{target}\") names no request in this run, so iteration {} ended.",
                        current.iteration + 1
                    ));
                }

                found
            }
        };

        self.next = match index {
            Some(index) => Some(Position {
                iteration: current.iteration,
                index,
            }),
            None => (current.iteration + 1 < self.iterations).then_some(Position {
                iteration: current.iteration + 1,
                index: 0,
            }),
        };

        warning
    }

    pub fn finish(&mut self) {
        self.next = None;
    }
}

/// The request a script chose last; the request's own scripts run after the
/// collection's, and post-response scripts after pre-request scripts.
pub(crate) fn chosen_request(scripts: &[ScriptReport]) -> Option<&NextRequest> {
    scripts
        .iter()
        .rev()
        .find_map(|report| report.next_request.as_ref())
}

/// What one request of a run did.
pub(crate) struct RunResult {
    pub position: Position,
    /// The method it went out with, which a pre-request script can change.
    pub method: Method,
    pub outcome: Outcome,
    pub scripts: Vec<ScriptReport>,
    /// Where the request went, once its variables resolved.
    pub url: Option<String>,
}

pub(crate) enum Outcome {
    Response {
        status: request::StatusCode,
        elapsed: Duration,
        response: Kept,
    },
    /// A pre-request script skipped the request.
    Skipped(String),
    /// The request could not be sent or did not complete.
    Failed(String),
}

/// Whether a result keeps its response to show after the run.
pub(crate) enum Kept {
    /// The response and the request as it went out. Its scripts' reports
    /// are the result's.
    Response(Arc<Execution>),
    /// Persist responses for a session is off.
    Off,
    /// The run keeps no more response bodies.
    OverLimit,
}

/// How many bytes of response bodies a run keeps, so a long run cannot fill
/// the memory with them.
pub(crate) const KEPT_BODY_LIMIT: usize = 256 * 1024 * 1024;

impl RunResult {
    /// `method` is the saved request's. `kept` is how many more response
    /// body bytes the run keeps, or None while responses are not kept;
    /// `logs` keeps the scripts' console output.
    pub fn new(
        position: Position,
        method: Method,
        result: Result<Execution, ExecutionFailure>,
        kept: Option<&mut usize>,
        logs: bool,
    ) -> Self {
        let mut method = method;
        let mut url = None;
        let (outcome, mut scripts) = match result {
            Ok(mut execution) => {
                if let Some(sent) = &execution.sent {
                    url = Some(crate::response_view::sent_url(sent));
                    method = sent.method;
                }
                let scripts = std::mem::take(&mut execution.scripts);
                let Response::Http(response) = &execution.response;
                let (status, elapsed, size) =
                    (response.status, execution.elapsed, response.body.len());
                let response = match kept {
                    None => Kept::Off,
                    Some(remaining) if size > *remaining => Kept::OverLimit,
                    Some(remaining) => {
                        *remaining -= size;
                        Kept::Response(Arc::new(execution))
                    }
                };

                (
                    Outcome::Response {
                        status,
                        elapsed,
                        response,
                    },
                    scripts,
                )
            }
            Err(failure) => {
                let outcome = match failure.error {
                    ExecutionError::Skipped { reason } => Outcome::Skipped(reason),
                    ExecutionError::Script { message } => Outcome::Failed(message),
                    error => Outcome::Failed(error.message_without_url()),
                };

                (outcome, failure.scripts)
            }
        };

        if !logs {
            for report in &mut scripts {
                report.logs.clear();
            }
        }

        Self {
            position,
            method,
            outcome,
            scripts,
            url,
        }
    }

    pub fn tests(&self) -> impl Iterator<Item = &request::ScriptTest> {
        self.scripts.iter().flat_map(|report| &report.tests)
    }

    /// A request that failed or whose script failed, as opposed to a test.
    pub fn is_error(&self) -> bool {
        matches!(self.outcome, Outcome::Failed(_))
            || self.scripts.iter().any(|report| report.error.is_some())
    }
}

/// A run's counts so far, kept as results arrive, so showing them does not
/// take longer as the run grows.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Totals {
    /// Tests that passed and failed.
    pub passed: usize,
    pub failed: usize,
    /// Requests with a test that passed, and with one that failed.
    pub passing_requests: usize,
    pub failing_requests: usize,
    /// Requests that a pre-request script skipped.
    pub skipped: usize,
    /// Requests that could not be sent or whose scripts failed.
    pub errors: usize,
    /// Lines the scripts logged.
    pub logs: usize,
    responses: u32,
    elapsed: Duration,
}

impl Totals {
    pub fn add(&mut self, result: &RunResult) {
        let (mut passed, mut failed) = (0, 0);
        for test in result.tests() {
            match test.error {
                None => passed += 1,
                Some(_) => failed += 1,
            }
        }
        self.passed += passed;
        self.failed += failed;
        self.passing_requests += usize::from(passed > 0);
        self.failing_requests += usize::from(failed > 0);
        self.logs += result
            .scripts
            .iter()
            .map(|report| report.logs.len())
            .sum::<usize>();

        match result.outcome {
            Outcome::Response { elapsed, .. } => {
                self.responses += 1;
                self.elapsed += elapsed;
            }
            Outcome::Skipped(_) => self.skipped += 1,
            Outcome::Failed(_) => {}
        }

        self.errors += usize::from(result.is_error());
    }

    pub fn tests(&self) -> usize {
        self.passed + self.failed
    }

    /// The average response time.
    pub fn average(&self) -> Option<Duration> {
        (self.responses > 0).then(|| self.elapsed / self.responses)
    }
}

/// A duration as the runner writes every time: `539 ms`, `1.07 s` or
/// `2 min 05 s`.
pub(crate) fn duration_label(duration: Duration) -> String {
    let milliseconds = duration.as_millis();

    if milliseconds < 1000 {
        format!("{milliseconds} ms")
    } else if milliseconds < 60_000 {
        format!("{:.2} s", duration.as_secs_f64())
    } else {
        let seconds = duration.as_secs();
        format!("{} min {:02} s", seconds / 60, seconds % 60)
    }
}

/// A count with its thousands apart, such as `1,000,000`.
pub(crate) fn count_label(count: usize) -> String {
    let digits = count.to_string();
    let mut label = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            label.push(',');
        }
        label.push(digit);
    }

    label
}

/// What went wrong with a request that could not be sent, in a few words
/// when the HTTP client's message has a common cause. Other messages are
/// already short and stay as they are.
pub(crate) fn failure_summary(message: &str) -> &str {
    if !message.starts_with("HTTP transport failed") {
        return message;
    }

    let lower = message.to_lowercase();
    let causes = [
        ("connection refused", "Connection refused"),
        ("dns error", "Host not found"),
        ("failed to lookup address", "Host not found"),
        ("connection reset", "Connection reset"),
        ("certificate", "Certificate not trusted"),
        ("timed out", "Timed out"),
    ];

    causes
        .into_iter()
        .find(|(cause, _)| lower.contains(cause))
        .map_or(message, |(_, summary)| summary)
}
