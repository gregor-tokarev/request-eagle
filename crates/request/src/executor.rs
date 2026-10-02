use std::{
    future::Future,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use bytes::Bytes;

use http_client::http::header::WWW_AUTHENTICATE;

use crate::{
    Auth, Body, CookieJar, EventStream, Execution, ExecutionError, Field, HttpRequest,
    RequestPreferences, RequestVariables, Response, StatusCode, http::HttpExecutor, scripts,
};

/// How much of a raw body `Execution::sent` keeps.
const SENT_TEXT_LIMIT: usize = 64 * 1024;

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
        mut request: HttpRequest,
        variables: RequestVariables,
        mut events: Option<EventStream>,
    ) -> impl Future<Output = Result<Execution, ExecutionError>> + Send + 'static + use<> {
        let executor = self.clone();
        // Resolved with the request's other fields, after pre-request scripts.
        request.auth = variables.effective_auth(&request.auth);
        let opened = events.as_ref().map(|events| events.opened.clone());
        let timeout = match request.settings.timeout_ms {
            Some(0) => None,
            Some(timeout) => Some(Duration::from_millis(timeout)),
            None => self.timeout,
        };

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

                // Reading the body's files blocks.
                let (mut request, body) = smol::unblock(move || {
                    let body = request.encode_body();
                    (request, body)
                })
                .await;
                let body = body?.map(Bytes::from);
                authorize(&mut request, body.as_deref())?;

                let sent_at = Instant::now();

                // Only a post-response script reads the sent body. Otherwise
                // HTTP owns the upload and releases it before the download.
                let has_post_script = !request.scripts.post_response.trim().is_empty()
                    || !state.collection_post_response.trim().is_empty();
                let post_body = if has_post_script { body.clone() } else { None };

                // Encoding moved raw text into the body's bytes. Keep only its
                // start for the request as sent, so a large upload is not
                // copied again.
                let sent_text = match (&request.body, &body) {
                    (Some(Body::Raw { .. }), Some(bytes)) => Some(
                        String::from_utf8_lossy(&bytes[..bytes.len().min(SENT_TEXT_LIMIT)])
                            .into_owned(),
                    ),
                    _ => None,
                };

                // Digest may send the body again; otherwise HTTP owns it.
                let digest_body = matches!(request.auth, Auth::Digest(_))
                    .then(|| body.clone())
                    .flatten();
                let (mut response, mut url) = executor
                    .http
                    .execute(&request, body, events.as_mut())
                    .await?;

                if let Some(answer) =
                    answer_digest(&request, &response, &url, digest_body.as_deref())
                {
                    request.headers.push(Field::new("Authorization", answer));
                    (response, url) = executor
                        .http
                        .execute(&request, digest_body, events.as_mut())
                        .await?;
                }
                state.response_url = Some(url.into());

                let mut sent = request.clone();
                if let (Some(Body::Raw { text, .. }), Some(sent_text)) = (&mut sent.body, sent_text)
                {
                    *text = sent_text;
                }
                let execution = Execution {
                    response: Response::Http(response),
                    elapsed: sent_at.elapsed(),
                    scripts: std::mem::take(&mut reports),
                    sent: Some(sent),
                };

                Ok((request, post_body, state, execution))
            };

            let (request, body, state, execution) = match timeout {
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

/// Add the credentials of the request's resolved authorization.
fn authorize(request: &mut HttpRequest, body: Option<&[u8]>) -> Result<(), ExecutionError> {
    let form = match &request.body {
        Some(Body::UrlEncoded { fields }) => fields.clone(),
        _ => Vec::new(),
    };

    crate::auth::authorize(
        &request.auth,
        request.method.as_str(),
        &request.path,
        &mut request.query,
        &mut request.headers,
        body.unwrap_or_default(),
        &form,
    )
    .map_err(ExecutionError::Auth)
}

/// The Authorization header that answers a Digest challenge in the
/// response, unless the request set its own. Only the address the request
/// was sent to is answered: a redirect may lead to another server, which
/// must not learn the credentials, or change the method and body.
fn answer_digest(
    request: &HttpRequest,
    response: &crate::HttpResponse,
    url: &url::Url,
    body: Option<&[u8]>,
) -> Option<String> {
    let Auth::Digest(credentials) = &request.auth else {
        return None;
    };
    if response.status != StatusCode::UNAUTHORIZED
        || Field::enabled(&request.headers)
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    {
        return None;
    }

    let mut sent = url::Url::parse(&request.path).ok()?;
    sent.set_fragment(None);
    let query = Field::pairs(&request.query);
    if !query.is_empty() {
        sent.query_pairs_mut().extend_pairs(query);
    }
    if sent != *url {
        return None;
    }

    credentials.answer_digest(
        response
            .headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .filter_map(|value| value.to_str().ok()),
        request.method.as_str(),
        url,
        body.unwrap_or_default(),
    )
}
