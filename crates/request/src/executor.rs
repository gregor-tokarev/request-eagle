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
            let started_at = Instant::now();
            let run = async {
                let response = match request {
                    Request::Http(request) => Response::Http(executor.http.execute(request).await?),
                };

                Ok(Execution {
                    response,
                    elapsed: started_at.elapsed(),
                })
            };

            match executor.timeout {
                Some(timeout) => {
                    smol::future::or(run, async {
                        smol::Timer::after(timeout).await;

                        Err(ExecutionError::Timeout { timeout })
                    })
                    .await
                }
                None => run.await,
            }
        }
    }
}
