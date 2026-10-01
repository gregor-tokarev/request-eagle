use std::{
    future::Future,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use bytes::Bytes;

use crate::{
    CookieJar, EventStream, Execution, ExecutionError, HttpRequest, RequestPreferences,
    RequestVariables, Response, http::HttpExecutor, scripts,
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

    /// Store the cookies that responses set in `jar` and send them with later
    /// requests to the same sites, including script requests. Executors that
    /// share a jar share its cookies. Ignored when the preferences turn the
    /// cookie jar off.
    pub fn with_cookie_jar(mut self, jar: CookieJar) -> Self {
        self.http = self.http.with_cookie_jar(jar);
        self
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
        self.run(request, variables, None)
    }

    /// Like `execute`, but an event-stream response reports its events through
    /// `events` as they arrive. Once it opens, the request timeout no longer
    /// applies: the stream lasts until the server ends it or it is stopped.
    pub fn execute_streaming(
        &self,
        request: HttpRequest,
        variables: RequestVariables,
        events: EventStream,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<> {
        self.run(request, variables, Some(events))
    }

    fn run(
        &self,
        request: HttpRequest,
        variables: RequestVariables,
        mut events: Option<EventStream>,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<> {
        let executor = self.clone();
        let opened = events.as_ref().map(|events| events.opened.clone());

        async move {
            let cancellation = scripts::Cancellation::new();
            let mut reports = Vec::new();
            let run = async {
                let (mut request, mut state, pre_reports) = scripts::pre_request(
                    request,
                    variables,
                    executor.clone(),
                    cancellation.0.clone(),
                )
                .await?;
                reports = pre_reports;

                let sent_at = Instant::now();
                let body = request.body.take().map(Bytes::from);

                // Only a post-response script reads the sent body. Otherwise
                // HTTP owns the upload and releases it before the download.
                let has_post_script = !request.scripts.post_response.trim().is_empty()
                    || !state.collection_post_response.trim().is_empty();
                let post_body = if has_post_script { body.clone() } else { None };

                let (response, url) = executor
                    .http
                    .execute(&request, body, events.as_mut())
                    .await?;
                state.response_url = Some(url.into());
                let execution = Execution {
                    response: Response::Http(response),
                    elapsed: sent_at.elapsed(),
                    scripts: std::mem::take(&mut reports),
                };

                Ok((request, post_body, state, execution))
            };

            let (request, body, state, execution) = match executor.timeout {
                Some(timeout) => {
                    smol::future::or(run, async {
                        smol::Timer::after(timeout).await;

                        if opened
                            .as_ref()
                            .is_some_and(|opened| opened.load(Ordering::SeqCst))
                        {
                            std::future::pending::<()>().await;
                        }

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
