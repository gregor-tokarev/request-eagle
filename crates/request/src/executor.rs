use std::{
    future::Future,
    time::{Duration, Instant},
};

use crate::{Execution, ExecutionError, Request, RequestPreferences, Response, http::HttpExecutor};

/// Reusable protocol dispatcher with a connection pool and a settings snapshot.
/// Construct a new executor when request preferences change.
#[derive(Clone)]
pub struct RequestExecutor {
    http: HttpExecutor,
    timeout: Option<Duration>,
}

impl RequestExecutor {
    pub fn new(preferences: &RequestPreferences) -> Result<Self, ExecutionError> {
        Ok(Self {
            http: HttpExecutor::new(preferences)?,
            timeout: (preferences.timeout_ms != 0)
                .then(|| Duration::from_millis(preferences.timeout_ms)),
        })
    }

    /// Takes a snapshot immediately, so editing the source cannot change a run.
    /// Accepts owned or borrowed `Request` and `HttpRequest` values.
    /// Dropping the returned future cancels the run, including a pending body read.
    pub fn execute<R: Into<Request>>(
        &self,
        request: R,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<R> {
        let request = request.into();
        let executor = self.clone();

        async move {
            let cancellation = crate::scripts::Cancellation::new();
            let run = async {
                match request {
                    Request::Http(request) => {
                        let (request, variables, scripts) =
                            crate::scripts::pre_request(request, cancellation.0.clone()).await?;
                        let sent_at = Instant::now();
                        let post_request = (!request.scripts.post_response.trim().is_empty())
                            .then(|| request.clone());
                        let response = executor.http.execute(request).await.map_err(|error| {
                            if scripts.is_empty() {
                                error
                            } else {
                                ExecutionError::ScriptedRequest {
                                    source: Box::new(error),
                                    reports: scripts.clone(),
                                }
                            }
                        })?;
                        let execution = Execution {
                            response: Response::Http(response),
                            elapsed: sent_at.elapsed(),
                            scripts,
                        };

                        Ok((post_request, variables, execution))
                    }
                }
            };

            let (post_request, variables, execution) = match executor.timeout {
                Some(timeout) => {
                    smol::future::or(run, async {
                        smol::Timer::after(timeout).await;

                        Err(ExecutionError::Timeout { timeout })
                    })
                    .await
                }
                None => run.await,
            }?;

            // Once the response is complete, its script uses the separate script
            // deadline. A request timeout must not discard a received response.
            match post_request {
                Some(request) => Ok(crate::scripts::post_response(
                    request,
                    variables,
                    execution,
                    cancellation.0.clone(),
                )
                .await),
                None => Ok(execution),
            }
        }
    }
}
