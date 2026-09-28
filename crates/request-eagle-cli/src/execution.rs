use anyhow::{Context as _, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use environment::VariableValues;
use request::{HttpRequest, Method, RequestExecutor, RequestScripts, RequestVariables, Response};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

use crate::commands::{Body, Method as InputMethod, RequestInput};

pub async fn run(
    root: &Path,
    preferences: &preferences::PreferencesFile,
    path: &Path,
    trust_scripts: bool,
    variables: HashMap<String, String>,
    timeout_ms: Option<u64>,
) -> Result<Value> {
    let (registry, lock) = crate::collections::load(root)?;
    let file = registry.file(path).context("Unknown saved request path")?;
    let request::Request::Http(request) = file.request.clone();
    if !request.scripts.is_empty() && !trust_scripts {
        bail!("Read the saved request scripts, then set trust_scripts=true to approve this run");
    }

    let collection = registry
        .collections()
        .iter()
        .find(|collection| path.starts_with(&collection.path))
        .context("Unknown collection")?;
    let mut values = VariableValues {
        environment: collection.local_env().entries.clone(),
    };
    values.environment.extend(variables);
    drop(lock);
    let variables = RequestVariables::with_environment_session(values, None, Default::default());
    let mut settings = preferences.request_preferences().await?;
    if let Some(timeout) = timeout_ms {
        settings.timeout_ms = timeout;
    }

    let executor = RequestExecutor::new(&settings)?;
    let execution = executor.execute_with_variables(request, variables).await?;
    let Response::Http(response) = execution.response;
    let headers = response.headers.iter().map(|(name, value)| json!({
        "name": name.as_str(), "value": value.to_str().ok(), "value_base64": STANDARD.encode(value.as_bytes()),
    })).collect::<Vec<_>>();
    let scripts = execution.scripts.iter().map(|report| json!({
        "phase": match report.phase { request::ScriptPhase::PreRequest => "pre_request", request::ScriptPhase::PostResponse => "post_response" },
        "error": report.error,
        "tests": report.tests.iter().map(|test| json!({"name": test.name, "passed": test.error.is_none(), "error": test.error})).collect::<Vec<_>>(),
        "logs": report.logs.iter().map(|log| json!({"level": log.level, "message": log.message})).collect::<Vec<_>>(),
    })).collect::<Vec<_>>();

    Ok(json!({
        "path": path, "status": response.status.as_u16(), "http_version": format!("{:?}", response.version),
        "headers": headers, "body_base64": STANDARD.encode(&response.body), "body_bytes": response.body.len(),
        "elapsed_ms": execution.elapsed.as_secs_f64() * 1000., "scripts": scripts,
    }))
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
            query: (!input.query.is_empty()).then_some(input.query),
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
            query: request.query.clone().unwrap_or_default(),
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
