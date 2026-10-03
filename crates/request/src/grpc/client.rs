use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime},
};

use futures::channel::mpsc::unbounded;
use http_client::http::{HeaderMap, HeaderName, HeaderValue, uri::PathAndQuery};
use prost_reflect::MethodDescriptor;
use tonic::metadata::MetadataMap;

use super::{
    GrpcCall, GrpcDefinition, GrpcError, GrpcEvent, GrpcEvents, GrpcFailure, GrpcRequest,
    MethodKind, ServiceDefinition,
    call::{self, CallTarget},
    reflection,
    transport::{self, Target},
};
use crate::{
    Auth, CookieJar, Field, RequestExecutor, RequestPreferences, RequestVariables, ScriptReport,
    auth::Credential, scripts::CallScripts,
};

/// Server reflection gives up here unless the request timeout is shorter.
const REFLECTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Loads service definitions and starts calls with a settings snapshot.
/// Construct a new client when request preferences change.
#[derive(Clone)]
pub struct GrpcClient {
    verify_certificates: bool,
    /// Largest response message; zero preferences mean no limit.
    max_message_bytes: usize,
    /// Applies to unary calls and reflection. Streams stay open until the
    /// server ends them or the call is cancelled.
    timeout: Option<Duration>,
    preferences: RequestPreferences,
    /// Holds the cookies of scripts' HTTP requests.
    cookies: Option<CookieJar>,
    /// Sends the HTTP requests of scripts, created when a script first runs.
    script_executor: Arc<OnceLock<Result<RequestExecutor, String>>>,
}

impl GrpcClient {
    pub fn new(preferences: &RequestPreferences) -> Self {
        Self {
            verify_certificates: preferences.ssl_certificate_verification,
            max_message_bytes: message_limit(preferences.max_response_size_mb),
            timeout: (preferences.timeout_ms != 0)
                .then(|| Duration::from_millis(preferences.timeout_ms)),
            preferences: preferences.clone(),
            cookies: None,
            script_executor: Arc::default(),
        }
    }

    /// Share `jar` with the HTTP requests that scripts send, as
    /// `RequestExecutor::with_cookie_jar` does.
    pub fn with_cookie_jar(mut self, jar: CookieJar) -> Self {
        self.cookies = Some(jar);
        self
    }

    /// Where and how to connect, with the request's TLS settings applied.
    fn target(&self, request: &GrpcRequest) -> Result<Target, GrpcError> {
        let mut target = Target::parse(&request.url, request.tls)?;
        let server_name = request.settings.server_name.trim();

        target.verify_certificates = request
            .settings
            .verify_certificates
            .unwrap_or(self.verify_certificates);
        target.server_name = (!server_name.is_empty()).then(|| server_name.to_owned());
        target.ca_certificates = self.preferences.ca_certificates.clone();
        target.client_certificate = target
            .tls
            .then(|| {
                crate::certificates::client_certificate(
                    &self.preferences.client_certificates,
                    &target.host,
                    target.port,
                )
            })
            .flatten()
            .cloned();

        Ok(target)
    }

    /// The request's timeout, or the preference's when it has none.
    fn timeout(&self, request: &GrpcRequest) -> Option<Duration> {
        match request.settings.timeout_ms {
            Some(0) => None,
            Some(timeout) => Some(Duration::from_millis(timeout)),
            None => self.timeout,
        }
    }

    /// Load the request's services, from its `.proto` file or the server.
    /// Relative `.proto` and import paths resolve from `collection`.
    pub fn load_definition(
        &self,
        request: &GrpcRequest,
        variables: &RequestVariables,
        collection: Option<&Path>,
    ) -> impl Future<Output = Result<ServiceDefinition, GrpcError>> + Send + 'static + use<> {
        let request_timeout = self.timeout(request);
        let prepared = match &request.definition {
            GrpcDefinition::ProtoFile { path, import_paths } => {
                let resolve = |path: &Path| resolve_path(path, collection);

                resolve(path).and_then(|path| {
                    let import_paths = import_paths
                        .iter()
                        .map(|path| resolve(path))
                        .collect::<Result<Vec<_>, _>>()?;

                    Ok(Definition::ProtoFile(path, import_paths))
                })
            }
            GrpcDefinition::Reflection => variables
                .resolve_grpc_target(request)
                .map_err(GrpcError::Variables)
                .and_then(|request| {
                    Ok(Definition::Reflection(
                        Box::new(self.target(&request)?),
                        metadata(&request.metadata, &request.auth)?,
                    ))
                }),
        };

        async move {
            match prepared? {
                Definition::ProtoFile(path, import_paths) => {
                    ServiceDefinition::from_proto_file(&path, &import_paths)
                }
                Definition::Reflection(target, metadata) => {
                    let timeout = request_timeout.map_or(REFLECTION_TIMEOUT, |timeout| {
                        timeout.min(REFLECTION_TIMEOUT)
                    });

                    on_runtime(async move {
                        let load = async {
                            let channel = transport::connect(&target, request_timeout).await?;

                            // A TLS server resets a plaintext connection.
                            match reflection::load(channel, metadata).await {
                                Err(GrpcError::Connect(_))
                                    if transport::expects_tls(&target).await =>
                                {
                                    Err(GrpcError::TlsRequired)
                                }
                                result => result,
                            }
                        };

                        tokio::time::timeout(timeout, load)
                            .await
                            .unwrap_or(Err(GrpcError::Timeout { timeout }))
                    })
                    .await
                }
            }
        }
    }

    /// Run the request's Before invoke script, then start the call. Unary
    /// and server streaming methods send the request's message right away;
    /// streaming requests wait for `GrpcCall::send`. Dropping the future
    /// interrupts the script.
    pub fn invoke(
        &self,
        request: &GrpcRequest,
        variables: RequestVariables,
        definition: &ServiceDefinition,
    ) -> impl Future<Output = Result<(GrpcCall, GrpcEvents), GrpcFailure>> + Send + 'static + use<>
    {
        let client = self.clone();
        let definition = definition.clone();
        // An unknown method fails before any script runs.
        let method = method(request, &definition).map(|_| ());
        let prepare = self.prepare(request, variables);

        async move {
            method?;

            client.start(prepare.await?, &definition)
        }
    }

    /// Run the request's Before invoke script, which can change the call and
    /// set the variables it resolves with. Load the service definition for
    /// the prepared call, since reflection may need them, then start it.
    /// Dropping the future interrupts the script.
    pub fn prepare(
        &self,
        request: &GrpcRequest,
        mut variables: RequestVariables,
    ) -> impl Future<Output = Result<PreparedCall, GrpcFailure>> + Send + 'static + use<> {
        let client = self.clone();
        let mut request = request.clone();

        async move {
            if request.method.trim().is_empty() {
                return Err(GrpcError::MissingMethod.into());
            }

            let mut scripts = None;
            let mut report = None;

            if !request.scripts.is_empty() {
                let executor = client.script_executor()?;
                let mut call_scripts =
                    CallScripts::new(std::mem::take(&mut request.scripts), &variables, executor);
                report = call_scripts
                    .before_invoke(&mut request, &mut variables)
                    .await?;
                scripts = Some(call_scripts);
            }

            Ok(PreparedCall {
                request,
                variables,
                report,
                scripts,
            })
        }
    }

    fn script_executor(&self) -> Result<RequestExecutor, GrpcError> {
        self.script_executor
            .get_or_init(|| {
                let executor =
                    RequestExecutor::new(&self.preferences).map_err(|error| error.to_string())?;

                Ok(match &self.cookies {
                    Some(jar) => executor.with_cookie_jar(jar.clone()),
                    None => executor,
                })
            })
            .clone()
            .map_err(GrpcError::ScriptSetup)
    }

    /// Resolve and send a prepared call. Its failures keep the Before invoke
    /// script's results.
    pub fn start(
        &self,
        call: PreparedCall,
        definition: &ServiceDefinition,
    ) -> Result<(GrpcCall, GrpcEvents), GrpcFailure> {
        let PreparedCall {
            request,
            variables,
            report,
            scripts,
        } = call;

        self.open(&request, variables, definition, report.clone(), scripts)
            .map_err(|error| GrpcFailure {
                error,
                scripts: report.into_iter().collect(),
            })
    }

    /// `report` is the Before invoke script's, and `scripts` follow the
    /// call's events when they have On message or After response scripts.
    fn open(
        &self,
        request: &GrpcRequest,
        variables: RequestVariables,
        definition: &ServiceDefinition,
        report: Option<ScriptReport>,
        scripts: Option<CallScripts>,
    ) -> Result<(GrpcCall, GrpcEvents), GrpcError> {
        let (method, kind) = method(request, definition)?;
        let method_path = format!("/{}/{}", method.parent_service().full_name(), method.name());
        let path = PathAndQuery::try_from(method_path)
            .map_err(|error| GrpcError::UnknownMethod(error.to_string()))?;

        // A streaming request's messages resolve as they are sent, so an
        // unfinished draft does not stop the stream from opening.
        let (resolved, generated) = variables
            .resolve_grpc(request, !kind.streams_requests())
            .map_err(GrpcError::Variables)?;
        let target = self.target(&resolved)?;
        let metadata = metadata(&resolved.metadata, &resolved.auth)?;

        // Scripts that follow the call see each event before passing it on,
        // and the values generated for it.
        let scripts = scripts
            .filter(CallScripts::follow_events)
            .map(|mut scripts| {
                scripts.keep_generated(generated);
                scripts
            });
        let (events, raw) = unbounded();
        let (output, receiver, raw) = if scripts.is_some() {
            let (output, receiver) = unbounded();
            (output, receiver, Some(raw))
        } else {
            (events.clone(), raw, None)
        };

        if let Some(report) = report {
            let _ = output.unbounded_send(GrpcEvent::Script(report));
        }

        let (messages, outgoing) = unbounded();
        let include_defaults = request.settings.include_default_fields;
        let mut call = GrpcCall::new(
            kind,
            method.input(),
            messages,
            events.clone(),
            variables,
            include_defaults,
        );

        if !kind.streams_requests() {
            call.send_resolved(&resolved.message)?;
            call.end();
        }

        if let (Some(scripts), Some(raw)) = (scripts, raw) {
            let task = reqwest_client::runtime().spawn(scripts.forward(resolved, raw, output));
            call.tasks.push(task.abort_handle());
        }

        let timeout = self.timeout(request);
        let output = method.output();
        let max_message_bytes = match request.settings.max_response_message_mb {
            Some(megabytes) => message_limit(megabytes),
            None => self.max_message_bytes,
        };
        let started = Instant::now();
        let task = reqwest_client::runtime().spawn(async move {
            let run = async {
                let channel = transport::connect(&target, timeout).await?;
                let call_target = CallTarget {
                    channel,
                    path,
                    output,
                    metadata,
                    max_message_bytes,
                    include_defaults,
                };

                // A TLS server resets a plaintext call without a status.
                match call::run(call_target, outgoing, &events).await {
                    Err(status)
                        if transport::connection_error(&status).is_some()
                            && transport::expects_tls(&target).await =>
                    {
                        Err(GrpcError::TlsRequired)
                    }
                    result => Ok(result),
                }
            };

            // A stream stays open as long as the user keeps it open.
            let result = match timeout {
                Some(timeout) if !kind.streams_requests() && !kind.streams_responses() => {
                    tokio::time::timeout(timeout, run)
                        .await
                        .unwrap_or(Err(GrpcError::Timeout { timeout }))
                }
                _ => run.await,
            };

            let _ = events.unbounded_send(match result {
                Ok(result) => call::finish(result, started),
                Err(error) => GrpcEvent::Failed(error),
            });
        });
        call.tasks.push(task.abort_handle());

        Ok((call, receiver))
    }
}

/// A call whose Before invoke script has run. Its request and variables
/// are what the script left them.
pub struct PreparedCall {
    request: GrpcRequest,
    variables: RequestVariables,
    report: Option<ScriptReport>,
    scripts: Option<CallScripts>,
}

impl PreparedCall {
    pub fn request(&self) -> &GrpcRequest {
        &self.request
    }

    pub fn variables(&self) -> &RequestVariables {
        &self.variables
    }

    /// A failure to start the call, with the Before invoke script's results.
    pub fn fail(self, error: GrpcError) -> GrpcFailure {
        GrpcFailure {
            error,
            scripts: self.report.into_iter().collect(),
        }
    }
}

/// The selected method's descriptor and kind.
fn method(
    request: &GrpcRequest,
    definition: &ServiceDefinition,
) -> Result<(MethodDescriptor, MethodKind), GrpcError> {
    let path = request.method.trim();

    if path.is_empty() {
        return Err(GrpcError::MissingMethod);
    }

    let descriptor = definition
        .descriptor(path)
        .ok_or_else(|| GrpcError::UnknownMethod(path.to_owned()))?;
    let kind = definition
        .method(path)
        .map_or(MethodKind::Unary, |method| method.kind);

    Ok((descriptor, kind))
}

/// A limit in MiB as bytes; zero is unlimited.
fn message_limit(megabytes: u64) -> usize {
    match megabytes {
        0 => usize::MAX,
        megabytes => usize::try_from(megabytes.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX),
    }
}

enum Definition {
    ProtoFile(PathBuf, Vec<PathBuf>),
    Reflection(Box<Target>, MetadataMap),
}

fn resolve_path(path: &Path, collection: Option<&Path>) -> Result<PathBuf, GrpcError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    collection
        .map(|collection| collection.join(path))
        .ok_or_else(|| {
            GrpcError::ProtoFile(format!(
                "{} is relative to a collection; save the request or choose the file again",
                path.display()
            ))
        })
}

/// The request's metadata, with the credentials of its resolved
/// authorization.
fn metadata(fields: &[Field], auth: &Auth) -> Result<MetadataMap, GrpcError> {
    let mut headers = HeaderMap::new();
    let credentials = auth_metadata(fields, auth).map_err(GrpcError::Auth)?;

    let sent = Field::enabled(fields).map(|(name, value)| (name.to_owned(), value.to_owned()));
    for (name, value) in sent.chain(credentials) {
        let name = name.trim();

        if name.is_empty() {
            continue;
        }

        let key = HeaderName::try_from(name.to_ascii_lowercase())
            .map_err(|_| GrpcError::InvalidMetadata(format!("{name} is not a valid key")))?;
        let value = HeaderValue::try_from(value)
            .map_err(|_| GrpcError::InvalidMetadata(format!("the value of {name} is invalid")))?;
        headers.append(key, value);
    }

    Ok(MetadataMap::from_headers(headers))
}

/// The credentials of a resolved authorization as metadata, unless the
/// request's own metadata sets them.
pub(crate) fn auth_metadata(
    metadata: &[Field],
    auth: &Auth,
) -> Result<Vec<(String, String)>, String> {
    if auth.overridden_by_metadata(metadata) {
        return Ok(Vec::new());
    }

    Ok(auth
        .credentials(None, SystemTime::now())?
        .into_iter()
        .map(|credential| match credential {
            // A call has no query, so its parameters are metadata too.
            Credential::Header(name, value) | Credential::Query(name, value) => (name, value),
        })
        .collect())
}

/// Run on the shared Tokio runtime, which the gRPC transport requires.
/// Dropping the returned future cancels the work.
async fn on_runtime<T: Send + 'static>(
    future: impl Future<Output = Result<T, GrpcError>> + Send + 'static,
) -> Result<T, GrpcError> {
    struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

    impl<T> Future for AbortOnDrop<T> {
        type Output = Result<T, tokio::task::JoinError>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx)
        }
    }

    impl<T> Drop for AbortOnDrop<T> {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    AbortOnDrop(reqwest_client::runtime().spawn(future))
        .await
        .unwrap_or_else(|error| Err(GrpcError::Connect(error.to_string())))
}
