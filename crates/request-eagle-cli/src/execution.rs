use anyhow::{Context as _, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::StreamExt as _;
use request::{
    GrpcClient, GrpcDefinition, GrpcEvent, GrpcRequest, GrpcScripts, GrpcSettings, HttpRequest,
    Method, RequestExecutor, RequestPreferences, RequestScripts, RequestVariables, Response,
    ScriptPhase, ScriptReport,
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

use crate::commands::{Body, GrpcProtocol, GrpcRequestInput, Method as InputMethod, RequestInput};

pub async fn run(
    root: &Path,
    preferences: &preferences::PreferencesFile,
    path: &Path,
    trust_scripts: bool,
    variables: HashMap<String, String>,
    timeout_ms: Option<u64>,
) -> Result<Value> {
    let registry = crate::collections::load(root)?;
    let file = registry.file(path).context("Unknown saved request path")?;
    let collection = registry
        .collections()
        .iter()
        .find(|collection| path.starts_with(&collection.path))
        .context("Unknown collection")?;
    let request = match file.request.clone() {
        request::Request::Http(request) => request,
        request::Request::Grpc(request) => {
            if !request.scripts.is_empty() && !trust_scripts {
                bail!(
                    "Read the saved request's scripts, then set trust_scripts=true to approve this run"
                );
            }

            let collection_values = collection.local_env().entries.clone();
            let mut settings = preferences.request_preferences().await?;
            if let Some(timeout) = timeout_ms {
                settings.timeout_ms = timeout;
            }

            return run_grpc(
                path,
                &collection.path,
                request,
                collection_values,
                variables,
                &settings,
            )
            .await;
        }
        request::Request::WebSocket(_) => {
            bail!(
                "requests.run sends HTTP and gRPC requests. Open WebSocket requests in the app to connect"
            )
        }
    };
    if (!request.scripts.is_empty() || !collection.scripts().is_empty()) && !trust_scripts {
        bail!(
            "Read the saved request and collection scripts, then set trust_scripts=true to approve this run"
        );
    }

    // Variables passed to the command override the collection's, like an
    // active environment.
    let variables = RequestVariables::with_environment_session(
        collection.local_env().entries.clone(),
        variables,
        None,
        Default::default(),
    )
    .with_collection_scripts(Ok(collection.scripts().clone()));
    let mut settings = preferences.request_preferences().await?;
    if let Some(timeout) = timeout_ms {
        settings.timeout_ms = timeout;
    }

    let executor = RequestExecutor::new(&settings)?;
    let execution = executor.execute(request, variables).await?;
    let Response::Http(response) = execution.response;
    let headers = response.headers.iter().map(|(name, value)| json!({
        "name": name.as_str(), "value": value.to_str().ok(), "value_base64": STANDARD.encode(value.as_bytes()),
    })).collect::<Vec<_>>();
    let scripts = execution
        .scripts
        .iter()
        .map(script_json)
        .collect::<Vec<_>>();

    Ok(json!({
        "path": path, "status": response.status.as_u16(), "http_version": format!("{:?}", response.version),
        "headers": headers, "body_base64": STANDARD.encode(&response.body), "body_bytes": response.body.len(),
        "elapsed_ms": execution.elapsed.as_secs_f64() * 1000., "scripts": scripts,
    }))
}

/// Invoke a saved gRPC method, sending its message once, and collect the
/// stream until the server's status.
async fn run_grpc(
    path: &Path,
    collection: &Path,
    request: GrpcRequest,
    collection_values: HashMap<String, String>,
    values: HashMap<String, String>,
    settings: &RequestPreferences,
) -> Result<Value> {
    let client = GrpcClient::new(settings);
    let variables = RequestVariables::with_environment_session(
        collection_values,
        values,
        None,
        Default::default(),
    );
    // Before invoke runs first, as it can set variables reflection needs.
    let prepared = client.prepare(&request, variables).await?;
    let definition = client
        .load_definition(prepared.request(), prepared.variables(), Some(collection))
        .await?;
    let (mut call, mut events) = client.start(prepared, &definition)?;

    if call.kind.streams_requests() {
        call.send(&request.message)?;
        call.end();
    }

    let mut messages = Vec::new();
    let mut metadata = Vec::new();
    let mut scripts = Vec::new();
    // Streams have no deadline in the app; here the command must return.
    let deadline = std::time::Duration::from_millis(match settings.timeout_ms {
        0 => 60_000,
        timeout => timeout,
    });
    let ends_at = std::time::Instant::now() + deadline;

    while let Some(event) = smol::future::or(events.next(), async {
        smol::Timer::at(ends_at).await;
        None
    })
    .await
    {
        match event {
            GrpcEvent::Metadata(pairs) => metadata = pairs,
            GrpcEvent::Sent(message) => messages.push(json!({"direction": "sent", "message": serde_json::from_str::<Value>(&message.json)?})),
            GrpcEvent::Received(message) => messages.push(json!({"direction": "received", "message": serde_json::from_str::<Value>(&message.json)?})),
            GrpcEvent::Finished { status, trailers, elapsed } => {
                return Ok(json!({
                    "path": path, "protocol": "grpc", "method": request.method,
                    "status": {"code": status.code, "name": status.name(), "message": status.message},
                    "metadata": metadata, "trailers": trailers, "messages": messages,
                    "elapsed_ms": elapsed.as_secs_f64() * 1000., "scripts": scripts,
                }));
            }
            GrpcEvent::Failed(error) => return Err(error.into()),
            GrpcEvent::Script(report) => scripts.push(script_json(&report)),
        }
    }

    bail!("The gRPC call did not finish within {deadline:?}; set timeout_ms to wait longer")
}

fn script_json(report: &ScriptReport) -> Value {
    json!({
        "phase": match report.phase {
            ScriptPhase::PreRequest => "pre_request",
            ScriptPhase::PostResponse => "post_response",
            ScriptPhase::BeforeInvoke => "before_invoke",
            ScriptPhase::OnMessage => "on_message",
            ScriptPhase::AfterResponse => "after_response",
        },
        "collection": report.collection,
        "message": report.message,
        "error": report.error,
        "tests": report.tests.iter().map(|test| json!({"name": test.name, "passed": test.error.is_none(), "error": test.error})).collect::<Vec<_>>(),
        "logs": report.logs.iter().map(|log| json!({"level": log.level, "message": log.message})).collect::<Vec<_>>(),
    })
}

impl From<GrpcRequestInput> for GrpcRequest {
    fn from(input: GrpcRequestInput) -> Self {
        Self {
            url: input.url,
            tls: input.tls,
            method: input.method,
            message: input.message,
            metadata: input.metadata,
            definition: match input.proto_file {
                Some(path) => GrpcDefinition::ProtoFile {
                    path,
                    import_paths: input.import_paths,
                },
                None => GrpcDefinition::Reflection,
            },
            settings: GrpcSettings {
                verify_certificates: input.verify_certificates,
                server_name: input.server_name,
                include_default_fields: input.include_default_fields.unwrap_or(true),
                max_response_message_mb: input.max_response_message_mb,
            },
            scripts: GrpcScripts {
                before_invoke: input.before_invoke,
                on_message: input.on_message,
                after_response: input.after_response,
            },
        }
    }
}

impl From<&GrpcRequest> for GrpcRequestInput {
    fn from(request: &GrpcRequest) -> Self {
        let (proto_file, import_paths) = match &request.definition {
            GrpcDefinition::ProtoFile { path, import_paths } => {
                (Some(path.clone()), import_paths.clone())
            }
            GrpcDefinition::Reflection => (None, Vec::new()),
        };

        Self {
            protocol: GrpcProtocol::Grpc,
            url: request.url.clone(),
            tls: request.tls,
            method: request.method.clone(),
            message: request.message.clone(),
            metadata: request.metadata.clone(),
            proto_file,
            import_paths,
            verify_certificates: request.settings.verify_certificates,
            server_name: request.settings.server_name.clone(),
            include_default_fields: (!request.settings.include_default_fields).then_some(false),
            max_response_message_mb: request.settings.max_response_message_mb,
            before_invoke: request.scripts.before_invoke.clone(),
            on_message: request.scripts.on_message.clone(),
            after_response: request.scripts.after_response.clone(),
        }
    }
}

impl From<RequestInput> for HttpRequest {
    fn from(input: RequestInput) -> Self {
        Self {
            method: match input.method {
                InputMethod::Get => Method::Get,
                InputMethod::Post => Method::Post,
                InputMethod::Put => Method::Put,
                InputMethod::Patch => Method::Patch,
                InputMethod::Head => Method::Head,
                InputMethod::Options => Method::Options,
                InputMethod::Delete => Method::Delete,
            },
            path: input.url,
            headers: input.headers,
            query: input.query,
            path_variables: input.path_variables,
            body: input.body.map(|body| match body {
                Body::Text(text) => text.into_bytes(),
                Body::Bytes(bytes) => bytes,
            }),
            scripts: RequestScripts {
                pre_request: input.pre_request,
                post_response: input.post_response,
            },
        }
    }
}

impl From<&HttpRequest> for RequestInput {
    fn from(request: &HttpRequest) -> Self {
        Self {
            method: match request.method {
                Method::Get => InputMethod::Get,
                Method::Post => InputMethod::Post,
                Method::Put => InputMethod::Put,
                Method::Patch => InputMethod::Patch,
                Method::Head => InputMethod::Head,
                Method::Options => InputMethod::Options,
                Method::Delete => InputMethod::Delete,
            },
            url: request.path.clone(),
            headers: request.headers.clone(),
            query: request.query.clone(),
            path_variables: request.path_variables.clone(),
            body: request
                .body
                .as_ref()
                .map(|bytes| match String::from_utf8(bytes.clone()) {
                    Ok(text) => Body::Text(text),
                    Err(_) => Body::Bytes(bytes.clone()),
                }),
            pre_request: request.scripts.pre_request.clone(),
            post_response: request.scripts.post_response.clone(),
        }
    }
}
