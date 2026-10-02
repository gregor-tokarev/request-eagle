use std::{path::PathBuf, time::Duration};

use gpui_kit::SharedString;
use request::{Execution, ExecutionError, HttpRequest, NextRequest, Response, ScriptReport};

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
    pub outcome: Outcome,
    pub scripts: Vec<ScriptReport>,
    /// Where the request went, once its variables resolved.
    pub url: Option<String>,
    /// The request as it went out, kept with the response.
    pub sent: Option<HttpRequest>,
}

pub(crate) enum Outcome {
    Response {
        status: request::StatusCode,
        elapsed: Duration,
        /// Kept while responses persist for the session.
        response: Option<request_history::Response>,
    },
    /// A pre-request script skipped the request.
    Skipped(String),
    /// The request could not be sent or did not complete.
    Failed(String),
}

impl RunResult {
    /// `persist` keeps the response's headers and body to show later; `logs`
    /// keeps the scripts' console output.
    pub fn new(
        position: Position,
        result: Result<Execution, ExecutionError>,
        persist: bool,
        logs: bool,
    ) -> Self {
        let mut url = None;
        let mut sent = None;
        let (outcome, mut scripts) = match result {
            Ok(mut execution) => {
                url = execution.sent.as_ref().map(crate::response_view::sent_url);
                if persist {
                    sent = execution.sent.take();
                }

                let Response::Http(response) = &execution.response;
                let outcome = Outcome::Response {
                    status: response.status,
                    elapsed: execution.elapsed,
                    response: persist.then(|| request_history::Response::new(&execution)),
                };

                (outcome, std::mem::take(&mut execution.scripts))
            }
            Err(error) => {
                // Earlier scripts' reports wrap a failure or skip in a later script.
                let (source, earlier) = match error {
                    ExecutionError::ScriptedRequest { source, reports } => (*source, reports),
                    error => (error, Vec::new()),
                };
                let (outcome, report) = match source {
                    ExecutionError::Skipped { reason, report } => {
                        (Outcome::Skipped(reason), Some(*report))
                    }
                    ExecutionError::Script { message, report } => {
                        (Outcome::Failed(message), Some(*report))
                    }
                    error => (Outcome::Failed(error.message_without_url()), None),
                };
                let scripts = if earlier.is_empty() {
                    report.into_iter().collect()
                } else {
                    earlier
                };

                (outcome, scripts)
            }
        };

        if !logs {
            for report in &mut scripts {
                report.logs.clear();
            }
        }

        Self {
            position,
            outcome,
            scripts,
            url,
            sent,
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
