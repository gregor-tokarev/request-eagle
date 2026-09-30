use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::channel::mpsc::unbounded;
use http_client::http::{HeaderMap, HeaderName, HeaderValue, uri::PathAndQuery};
use tonic::metadata::MetadataMap;

use super::{
    GrpcCall, GrpcDefinition, GrpcError, GrpcEvent, GrpcEvents, GrpcRequest, MethodKind,
    ServiceDefinition,
    call::{self, CallTarget},
    reflection,
    transport::{self, Target},
};
use crate::{RequestPreferences, RequestVariables};

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
}

impl GrpcClient {
    pub fn new(preferences: &RequestPreferences) -> Self {
        Self {
            verify_certificates: preferences.ssl_certificate_verification,
            max_message_bytes: message_limit(preferences.max_response_size_mb),
            timeout: (preferences.timeout_ms != 0)
                .then(|| Duration::from_millis(preferences.timeout_ms)),
        }
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

        Ok(target)
    }

    /// Load the request's services, from its `.proto` file or the server.
    /// Relative `.proto` and import paths resolve from `collection`.
    pub fn load_definition(
        &self,
        request: &GrpcRequest,
        variables: &RequestVariables,
        collection: Option<&Path>,
    ) -> impl Future<Output = Result<ServiceDefinition, GrpcError>> + Send + 'static + use<> {
        let client = self.clone();
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
                .resolve_grpc(request)
                .map_err(GrpcError::Variables)
                .and_then(|request| {
                    Ok(Definition::Reflection(
                        self.target(&request)?,
                        metadata(&request.metadata)?,
                    ))
                }),
        };

        async move {
            match prepared? {
                Definition::ProtoFile(path, import_paths) => {
                    ServiceDefinition::from_proto_file(&path, &import_paths)
                }
                Definition::Reflection(target, metadata) => {
                    let timeout = client.timeout.map_or(REFLECTION_TIMEOUT, |timeout| {
                        timeout.min(REFLECTION_TIMEOUT)
                    });

                    on_runtime(async move {
                        let load = async {
                            let channel = transport::connect(&target, client.timeout).await?;

                            reflection::load(channel, metadata).await
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

    /// Start a call. Unary and server streaming methods send the request's
    /// message right away; streaming requests wait for `GrpcCall::send`.
    pub fn invoke(
        &self,
        request: &GrpcRequest,
        variables: RequestVariables,
        definition: &ServiceDefinition,
    ) -> Result<(GrpcCall, GrpcEvents), GrpcError> {
        if request.method.trim().is_empty() {
            return Err(GrpcError::MissingMethod);
        }

        let resolved = variables
            .resolve_grpc(request)
            .map_err(GrpcError::Variables)?;
        let method = definition
            .descriptor(resolved.method.trim())
            .ok_or_else(|| GrpcError::UnknownMethod(resolved.method.trim().to_owned()))?;
        let method_path = format!("/{}/{}", method.parent_service().full_name(), method.name());
        let path = PathAndQuery::try_from(method_path)
            .map_err(|error| GrpcError::UnknownMethod(error.to_string()))?;
        let target = self.target(&resolved)?;
        let metadata = metadata(&resolved.metadata)?;
        let kind = definition
            .method(resolved.method.trim())
            .map_or(MethodKind::Unary, |method| method.kind);

        let (events, receiver) = unbounded();
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
            call.send(&request.message)?;
            call.end();
        }

        let client = self.clone();
        let output = method.output();
        let max_message_bytes = match request.settings.max_response_message_mb {
            Some(megabytes) => message_limit(megabytes),
            None => self.max_message_bytes,
        };
        let started = Instant::now();
        let task = reqwest_client::runtime().spawn(async move {
            let run = async {
                let channel = transport::connect(&target, client.timeout).await?;
                let target = CallTarget {
                    channel,
                    path,
                    output,
                    metadata,
                    max_message_bytes,
                    include_defaults,
                };

                Ok(call::run(target, outgoing, &events).await)
            };

            // A stream stays open as long as the user keeps it open.
            let result = match client.timeout {
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
        call.task = Some(task.abort_handle());

        Ok((call, receiver))
    }
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
    Reflection(Target, MetadataMap),
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

fn metadata(pairs: &[(String, String)]) -> Result<MetadataMap, GrpcError> {
    let mut headers = HeaderMap::new();

    for (name, value) in pairs {
        let name = name.trim();

        if name.is_empty() {
            continue;
        }

        let key = HeaderName::try_from(name.to_ascii_lowercase())
            .map_err(|_| GrpcError::InvalidMetadata(format!("{name} is not a valid key")))?;
        let value = HeaderValue::try_from(value.as_str())
            .map_err(|_| GrpcError::InvalidMetadata(format!("the value of {name} is invalid")))?;
        headers.append(key, value);
    }

    Ok(MetadataMap::from_headers(headers))
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
