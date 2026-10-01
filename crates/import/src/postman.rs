//! Postman Collection v2.0 and v2.1.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use collection::{ImportedCollection, ImportedItem};
use request::{HttpRequest, HttpSettings, Method, Request, RequestScripts};
use serde_json::Value;

use crate::{
    Import, ImportError,
    body::{multipart_form, set_content_type, url_encoded_form, url_encoded_form_script},
    document::{clean_name, text},
};

pub(crate) fn convert(document: &Value) -> Result<Import, ImportError> {
    let mut skipped = Vec::new();
    // Collection scripts become the collection's own scripts, so requests
    // inherit only its authorization.
    let inherited = Inherited {
        auth: own_auth(document),
        scripts: Scripts::default(),
    };
    let items = items(document, &inherited, &mut skipped);
    let scripts = scripts(document);

    Ok(Import {
        collection: ImportedCollection {
            name: clean_name(document["info"]["name"].as_str(), "Postman Collection"),
            variables: variables(document),
            scripts: RequestScripts {
                pre_request: join_scripts(&scripts.pre_request),
                post_response: join_scripts(&scripts.post_response),
            },
            items,
        },
        skipped,
    })
}

/// What a folder passes to the items inside it.
pub(crate) struct Inherited<'a> {
    pub(crate) auth: Option<&'a Value>,
    /// Request Eagle has no folder scripts, so each request runs its folders'
    /// scripts before its own, in the order Postman runs them.
    pub(crate) scripts: Scripts,
}

/// Scripts in the order Postman runs them, each kept separate until they are
/// joined for a request.
#[derive(Clone, Default)]
pub(crate) struct Scripts {
    pub(crate) pre_request: Vec<String>,
    pub(crate) post_response: Vec<String>,
}

impl Scripts {
    pub(crate) fn then(&self, next: Scripts) -> Scripts {
        Scripts {
            pre_request: [self.pre_request.clone(), next.pre_request].concat(),
            post_response: [self.post_response.clone(), next.post_response].concat(),
        }
    }
}

fn items(parent: &Value, inherited: &Inherited, skipped: &mut Vec<String>) -> Vec<ImportedItem> {
    let Some(children) = parent.get("item").and_then(Value::as_array) else {
        return Vec::new();
    };

    children
        .iter()
        .filter_map(|item| {
            let name = clean_name(item["name"].as_str(), "Untitled");

            if item.get("item").is_some() {
                let inherited = Inherited {
                    auth: own_auth(item).or(inherited.auth),
                    scripts: inherited.scripts.then(scripts(item)),
                };

                return Some(ImportedItem::Folder {
                    items: items(item, &inherited, skipped),
                    name,
                });
            }

            match request(item, inherited) {
                Some(request) => Some(ImportedItem::Request {
                    name,
                    request: Request::Http(request),
                }),
                None => {
                    skipped.push(name);
                    None
                }
            }
        })
        .collect()
}

/// The request, or `None` when Request Eagle cannot send its method.
pub(crate) fn request(item: &Value, inherited: &Inherited) -> Option<HttpRequest> {
    let request = &item["request"];
    let method = match request["method"]
        .as_str()
        .unwrap_or("GET")
        .to_uppercase()
        .as_str()
    {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        "DELETE" => Method::Delete,
        _ => return None,
    };

    // A request may be written as just its URL.
    let (path, path_variables) = match request {
        Value::String(url) => (url.clone(), Vec::new()),
        _ => url(&request["url"]),
    };

    let mut headers = pairs(&request["header"]);
    let mut query = Vec::new();
    let mut scripts = inherited.scripts.then(scripts(item));
    let body = body(&request["body"], &mut headers, &mut scripts);
    // Postman applies authorization after the pre-request scripts, which may
    // set the credentials it uses.
    authorize(
        own_auth(request).or(inherited.auth),
        &mut headers,
        &mut query,
        &mut scripts,
    );

    // The request's Settings tab in Postman.
    let behavior = &item["protocolProfileBehavior"];

    Some(HttpRequest {
        method,
        path,
        headers,
        body,
        query,
        path_variables,
        scripts: RequestScripts {
            pre_request: join_scripts(&scripts.pre_request),
            post_response: join_scripts(&scripts.post_response),
        },
        settings: HttpSettings {
            timeout_ms: None,
            follow_redirects: behavior["followRedirects"].as_bool(),
            verify_certificates: behavior["strictSSL"].as_bool(),
        },
    })
}

/// The URL as Postman shows it, and the values of its `:name` path
/// variables. Its query stays in the URL as written, so encoded values are
/// sent unchanged.
fn url(url: &Value) -> (String, Vec<(String, String)>) {
    let raw = match url {
        Value::String(raw) => return (raw.clone(), Vec::new()),
        Value::Object(_) => match url["raw"].as_str() {
            Some(raw) => raw.to_owned(),
            None => assemble_url(url),
        },
        _ => return (String::new(), Vec::new()),
    };

    let names: Vec<&str> = request::path_variables(&raw)
        .map(|(_, name)| name)
        .collect();
    let variables = pairs(&url["variable"])
        .into_iter()
        .filter(|(key, value)| !value.is_empty() && names.contains(&key.as_str()))
        .collect();

    (raw, variables)
}

/// A URL from the parts Postman stores beside, or instead of, the raw URL.
fn assemble_url(url: &Value) -> String {
    let join = |parts: &Value, separator: &str| match parts {
        Value::Array(parts) => parts
            .iter()
            .map(|part| text(Some(part.get("value").unwrap_or(part))))
            .collect::<Vec<_>>()
            .join(separator),
        part => text(Some(part)),
    };

    let mut assembled = String::new();
    if let Some(protocol) = url["protocol"].as_str() {
        assembled.push_str(&format!("{protocol}://"));
    }
    assembled.push_str(&join(&url["host"], "."));
    if !url["port"].is_null() {
        assembled.push_str(&format!(":{}", text(Some(&url["port"]))));
    }

    let path = join(&url["path"], "/");
    if !path.is_empty() && !path.starts_with('/') {
        assembled.push('/');
    }
    assembled.push_str(&path);

    let query = pairs(&url["query"]);
    if !query.is_empty() {
        let query: Vec<_> = query
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        assembled.push_str(&format!("?{}", query.join("&")));
    }

    assembled
}

fn body(
    body: &Value,
    headers: &mut Vec<(String, String)>,
    scripts: &mut Scripts,
) -> Option<Vec<u8>> {
    if body["disabled"].as_bool() == Some(true) {
        return None;
    }

    match body["mode"].as_str()? {
        "raw" => {
            let raw = body["raw"].as_str().filter(|raw| !raw.is_empty())?;
            let content_type = match body["options"]["raw"]["language"].as_str() {
                Some("json") => "application/json",
                Some("xml") => "application/xml",
                Some("html") => "text/html",
                Some("javascript") => "application/javascript",
                _ => "text/plain",
            };
            set_content_type(headers, content_type);

            Some(raw.as_bytes().to_vec())
        }
        "urlencoded" => {
            let fields = pairs(&body["urlencoded"]);
            if fields.is_empty() {
                return None;
            }
            scripts.pre_request.extend(url_encoded_form_script(&fields));

            Some(url_encoded_form(&fields, headers))
        }
        "formdata" => {
            // Files are not part of a saved request, so only text fields remain.
            let fields: Vec<_> = body["formdata"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|field| field["type"].as_str() != Some("file"))
                .filter(|field| field["disabled"].as_bool() != Some(true))
                .filter_map(|field| {
                    Some((field["key"].as_str()?.to_owned(), text(field.get("value"))))
                })
                .collect();
            (!fields.is_empty()).then(|| multipart_form(&fields, headers))
        }
        "graphql" => {
            let graphql = &body["graphql"];
            let query = serde_json::to_string(&text(graphql.get("query"))).ok()?;
            // Variables are JSON once their `{{variables}}` are filled in, so
            // they are kept as written.
            let variables = match &graphql["variables"] {
                Value::String(variables) => variables.trim().to_owned(),
                Value::Null => String::new(),
                variables => serde_json::to_string_pretty(variables).ok()?,
            };
            set_content_type(headers, "application/json");

            Some(if variables.is_empty() {
                format!("{{\n  \"query\": {query}\n}}").into_bytes()
            } else {
                format!("{{\n  \"query\": {query},\n  \"variables\": {variables}\n}}").into_bytes()
            })
        }
        _ => None,
    }
}

/// The item's own authorization, which `noauth` sets to none. Without one,
/// the item inherits its parent's.
pub(crate) fn own_auth(item: &Value) -> Option<&Value> {
    item.get("auth")
        .filter(|auth| !auth.is_null() && auth["type"].as_str() != Some("inherit"))
}

/// Applies an authorization as the headers or query parameters Postman sends.
pub(crate) fn authorize(
    auth: Option<&Value>,
    headers: &mut Vec<(String, String)>,
    query: &mut Vec<(String, String)>,
    scripts: &mut Scripts,
) {
    let Some(auth) = auth else { return };
    let kind = auth["type"].as_str().unwrap_or_default();
    let value = |key: &str| auth_value(auth, kind, key);
    let has_authorization = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));

    match kind {
        // Postman sends no token when it is empty.
        "bearer" if !has_authorization && !value("token").is_empty() => {
            headers.push(("Authorization".into(), format!("Bearer {}", value("token"))));
        }
        "oauth2" if !has_authorization && !value("accessToken").is_empty() => {
            headers.push((
                "Authorization".into(),
                format!("Bearer {}", value("accessToken")),
            ));
        }
        "basic" if !has_authorization => {
            let credentials = format!("{}:{}", value("username"), value("password"));
            if credentials.contains("{{") {
                // Variables are resolved when sending, so encode them then.
                let credentials = serde_json::to_string(&credentials).unwrap_or_default();
                scripts.pre_request.push(format!(
                    "pm.request.headers.upsert({{key: \"Authorization\", value: \"Basic \" + pm.encoding.base64Encode(pm.variables.replaceIn({credentials}))}});"
                ));
            } else {
                headers.push((
                    "Authorization".into(),
                    format!("Basic {}", STANDARD.encode(credentials)),
                ));
            }
        }
        "apikey" => {
            let pair = (value("key"), value("value"));
            if value("in") == "query" {
                query.push(pair);
            } else {
                headers.push(pair);
            }
        }
        _ => {}
    }
}

/// An authorization parameter: a key–value list in v2.1, an object in v2.0.
fn auth_value(auth: &Value, kind: &str, key: &str) -> String {
    match &auth[kind] {
        Value::Array(parameters) => text(
            parameters
                .iter()
                .find(|parameter| parameter["key"].as_str() == Some(key))
                .and_then(|parameter| parameter.get("value")),
        ),
        parameters => text(parameters.get(key)),
    }
}

/// Enabled key–value pairs, such as headers or form fields.
pub(crate) fn pairs(pairs: &Value) -> Vec<(String, String)> {
    pairs
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pair| pair["disabled"].as_bool() != Some(true))
        .filter_map(|pair| Some((pair["key"].as_str()?.to_owned(), text(pair.get("value")))))
        .collect()
}

fn variables(document: &Value) -> HashMap<String, String> {
    pairs(&document["variable"]).into_iter().collect()
}

/// The item's own scripts.
pub(crate) fn scripts(item: &Value) -> Scripts {
    let mut scripts = Scripts::default();

    for event in item["event"].as_array().into_iter().flatten() {
        if event["disabled"].as_bool() == Some(true) {
            continue;
        }

        let source = match &event["script"]["exec"] {
            Value::Array(lines) => lines
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
            Value::String(source) => source.clone(),
            _ => continue,
        };
        if source.trim().is_empty() {
            continue;
        }

        match event["listen"].as_str() {
            Some("prerequest") => scripts.pre_request.push(source),
            Some("test") => scripts.post_response.push(source),
            _ => {}
        }
    }

    scripts
}

/// Joins scripts that Postman runs one after another. Each keeps its own
/// block, so declarations with the same name in two of them do not collide.
pub(crate) fn join_scripts(scripts: &[String]) -> String {
    match scripts {
        [script] => script.clone(),
        scripts => scripts
            .iter()
            .map(|script| format!("{{\n{script}\n}}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}
