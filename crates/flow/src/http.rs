use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use environment::EnvironmentSession;
use request::{
    Auth, Body, Execution, Field, HttpRequest, RequestScripts, RequestVariables, Response,
};
use serde_json::{Map, Value, json};

/// A saved request an HTTP Request block sends, with what its variables
/// resolve from. Each send reads the session again, so a script's changes
/// reach the requests sent after it, as they do in the app.
#[derive(Clone)]
pub struct SavedRequest {
    pub name: String,
    /// Its files already resolved from its collection's directory.
    pub request: HttpRequest,
    /// The variables of the request's collection.
    pub collection: HashMap<String, String>,
    /// The active environment's variables.
    pub environment: HashMap<String, String>,
    /// Why the variables could not be read, if they could not.
    pub variables_error: Option<String>,
    /// The collection's scripts, which run around the request's own, or
    /// why they could not be read, which stops the request from sending.
    pub scripts: Result<RequestScripts, String>,
    /// What the request sends when it inherits its collection's authorization.
    pub auth: Auth,
    pub session: EnvironmentSession,
}

impl SavedRequest {
    /// The variables of one send, with the block's inputs over them.
    pub(crate) fn variables(&self, inputs: Vec<(String, String)>) -> RequestVariables {
        RequestVariables::with_environment_session(
            self.collection.clone(),
            self.environment.clone(),
            self.variables_error.clone(),
            self.session.clone(),
        )
        .with_collection_scripts(self.scripts.clone())
        .with_collection_auth(self.auth.clone())
        .with_local_values(inputs)
    }
}

/// The `{{variables}}` a request uses, in the order they first appear. These
/// are the inputs of an HTTP Request block that sends it. Generated values
/// such as `{{$guid}}` are left out.
pub fn request_variables(request: &HttpRequest) -> Vec<String> {
    let mut texts: Vec<&str> = vec![&request.path];
    for (name, value) in Field::enabled(&request.headers)
        .chain(Field::enabled(&request.query))
        .chain(
            request
                .path_variables
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
    {
        texts.push(name);
        texts.push(value);
    }
    match &request.body {
        Some(Body::Raw { text, .. }) => texts.push(text),
        Some(Body::UrlEncoded { fields }) => {
            for (name, value) in fields {
                texts.push(name);
                texts.push(value);
            }
        }
        Some(Body::Multipart { parts }) => {
            for part in parts {
                texts.push(&part.name);
                texts.push(&part.value);
            }
        }
        Some(Body::Binary { .. }) | None => {}
    }
    texts.push(&request.scripts.pre_request);
    texts.push(&request.scripts.post_response);

    let mut names: Vec<String> = Vec::new();
    for text in texts {
        for reference in text.split("{{").skip(1) {
            let Some((name, _)) = reference.split_once("}}") else {
                continue;
            };
            let name = name.trim();
            if !name.is_empty()
                && !name.starts_with(['$', '!'])
                && environment::valid_variable_name(name)
                && !names.iter().any(|existing| existing == name)
            {
                names.push(name.to_owned());
            }
        }
    }

    names
}

/// The text a value fills into a `{{variable}}`: a string as it is, other
/// values as JSON.
pub(crate) fn variable_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        value => value.to_string(),
    }
}

/// What an HTTP Request block sends on, as Postman Flows shapes it: the
/// parsed body, the status and headers, and the request's test results.
pub(crate) fn response_json(execution: &Execution) -> (bool, Value) {
    let Response::Http(response) = &execution.response;

    let mut headers = Map::new();
    for (name, value) in &response.headers {
        let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
        match headers.get_mut(name.as_str()) {
            Some(Value::String(existing)) => {
                existing.push_str(", ");
                existing.push_str(&value);
            }
            _ => {
                headers.insert(name.as_str().to_owned(), Value::String(value));
            }
        }
    }

    let (body, binary) = match std::str::from_utf8(&response.body) {
        Ok(text) => {
            let json = headers
                .get("content-type")
                .and_then(Value::as_str)
                .is_some_and(|content_type| content_type.contains("json"))
                || text.trim_start().starts_with(['{', '[']);
            let body = json
                .then(|| serde_json::from_str(text).ok())
                .flatten()
                .unwrap_or_else(|| Value::String(text.to_owned()));
            (body, false)
        }
        Err(_) => (Value::String(STANDARD.encode(&response.body)), true),
    };

    let tests: Vec<Value> = execution
        .scripts
        .iter()
        .flat_map(|report| &report.tests)
        .map(|test| json!({"name": test.name, "passed": test.error.is_none(), "error": test.error}))
        .collect();
    let elapsed = (execution.elapsed.as_secs_f64() * 10_000.).round() / 10.;

    let success = response.status.is_success();
    let value = json!({
        "body": body,
        "http": {
            "status": response.status.as_u16(),
            "headers": headers,
            "time": elapsed,
        },
        "tests": tests,
        "binary": binary,
    });

    (success, value)
}
