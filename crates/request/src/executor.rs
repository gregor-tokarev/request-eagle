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
        self.execute_inner(request, None)
    }

    /// Resolve collection and script variables together after the pre-request script.
    pub fn execute_with_variables<R: Into<Request>>(
        &self,
        request: R,
        variables: crate::RequestVariables,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<R> {
        self.execute_inner(request, Some(variables))
    }

    fn execute_inner<R: Into<Request>>(
        &self,
        request: R,
        variables: Option<crate::RequestVariables>,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<R> {
        let request = request.into();
        let executor = self.clone();

        async move {
            let cancellation = crate::scripts::Cancellation::new();
            let mut scripts = Vec::new();
            let run = async {
                match request {
                    Request::Http(request) => {
                        let (mut request, variables, reports) =
                            crate::scripts::pre_request_with_variables(
                                request,
                                cancellation.0.clone(),
                                variables,
                            )
                            .await?;
                        scripts = reports;

                        let sent_at = Instant::now();
                        let has_post_script = !request.scripts.post_response.trim().is_empty();
                        let body = request.body.take().map(bytes::Bytes::from);
                        let post_body = if has_post_script { body.clone() } else { None };
                        let response = executor.http.execute(&request, body).await?;
                        let post_request = has_post_script.then_some((request, post_body));
                        let execution = Execution {
                            response: Response::Http(response),
                            elapsed: sent_at.elapsed(),
                            scripts: std::mem::take(&mut scripts),
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
            }
            .map_err(|error| {
                if scripts.is_empty() {
                    error
                } else {
                    ExecutionError::ScriptedRequest {
                        source: Box::new(error),
                        reports: scripts,
                    }
                }
            })?;

            // Once the response is complete, its script uses the separate script
            // deadline. A request timeout must not discard a received response.
            match post_request {
                Some((request, body)) => Ok(crate::scripts::post_response(
                    request,
                    body,
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
