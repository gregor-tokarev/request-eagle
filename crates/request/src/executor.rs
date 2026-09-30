use std::{
    future::Future,
    time::{Duration, Instant},
};

use bytes::Bytes;

use crate::{
    Execution, ExecutionError, HttpRequest, RequestPreferences, RequestVariables, Response,
    http::HttpExecutor, scripts,
};

/// Reusable protocol dispatcher with a connection pool and a settings snapshot.
/// Construct a new executor when request preferences change.
#[derive(Clone)]
pub struct RequestExecutor {
    pub(crate) http: HttpExecutor,
    pub(crate) timeout: Option<Duration>,
}

impl RequestExecutor {
    pub fn new(preferences: &RequestPreferences) -> Result<Self, ExecutionError> {
        Ok(Self {
            http: HttpExecutor::new(preferences)?,
            timeout: (preferences.timeout_ms != 0)
                .then(|| Duration::from_millis(preferences.timeout_ms)),
        })
    }

    /// Run the request's scripts and send it, resolving variables after the
    /// pre-request scripts. The future owns its inputs, so editing the source
    /// cannot change a run. Dropping it cancels the run, including a pending
    /// body read.
    pub fn execute(
        &self,
        request: HttpRequest,
        variables: RequestVariables,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<> {
        let executor = self.clone();

        async move {
            let cancellation = scripts::Cancellation::new();
            let mut reports = Vec::new();
            let run = async {
                let (mut request, state, pre_reports) = scripts::pre_request(
                    request,
                    variables,
                    executor.clone(),
                    cancellation.0.clone(),
                )
                .await?;
                reports = pre_reports;

                let sent_at = Instant::now();
                let body = request.body.take().map(Bytes::from);
                let response = executor.http.execute(&request, body.clone()).await?;
                let execution = Execution {
                    response: Response::Http(response),
                    elapsed: sent_at.elapsed(),
                    scripts: std::mem::take(&mut reports),
                };

                Ok((request, body, state, execution))
            };

            let (request, body, state, execution) = match executor.timeout {
                Some(timeout) => {
                    smol::future::or(run, async {
                        smol::Timer::after(timeout).await;

                        Err(ExecutionError::Timeout { timeout })
                    })
                    .await
                }
                None => run.await,
            }
            .map_err(|error| {
                if reports.is_empty() {
                    error
                } else {
                    ExecutionError::ScriptedRequest {
                        source: Box::new(error),
                        reports,
                    }
                }
            })?;

            // Once the response is complete, its script uses the separate script
            // deadline. A request timeout must not discard a received response.
            Ok(scripts::post_response(
                request,
                body,
                state,
                execution,
                executor,
                cancellation.0.clone(),
            )
            .await)
        }
    }
}
