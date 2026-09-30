//! Postman Collection v2.0 and v2.1.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use collection::{ImportedCollection, ImportedItem};
use request::{HttpRequest, Method, Request, RequestScripts};
use serde_json::Value;

use crate::{
    Import, ImportError,
    body::{multipart_form, set_content_type, url_encoded_form},
    document::{clean_name, text},
};

pub(crate) fn convert(document: &Value) -> Result<Import, ImportError> {
    let mut skipped = Vec::new();
    // Collection scripts become the collection's own scripts, so requests
    // inherit only its authorization.
    let inherited = Inherited {
        auth: own_auth(document),
        scripts: RequestScripts::default(),
    };
    let items = items(document, &inherited, &mut skipped);

    Ok(Import {
        collection: ImportedCollection {
            name: clean_name(document["info"]["name"].as_str(), "Postman Collection"),
            variables: variables(document),
            scripts: scripts(document),
            items,
        },
        skipped,
    })
}

/// What a folder passes to the items inside it.
struct Inherited<'a> {
    auth: Option<&'a Value>,
    /// Request Eagle has no folder scripts, so each request runs its folders'
    /// scripts before its own, in the order Postman runs them.
    scripts: RequestScripts,
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
                    scripts: join_scripts(&inherited.scripts, &scripts(item)),
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
fn request(item: &Value, inherited: &Inherited) -> Option<HttpRequest> {
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
    let path = match request {
        Value::String(url) => url.clone(),
        _ => url(&request["url"]),
    };

    let mut headers = pairs(&request["header"]);
    let body = body(&request["body"], &mut headers);
    let mut query = Vec::new();
    let mut scripts = join_scripts(&inherited.scripts, &scripts(item));
    authorize(
        own_auth(request).or(inherited.auth),
        &mut headers,
        &mut query,
        &mut scripts,
    );

    Some(HttpRequest {
        method,
        path,
        headers,
        body,
        query,
        scripts,
    })
}

/// The URL as Postman shows it, with path variables filled in. Its query stays
/// in the URL as written, so encoded values are sent unchanged.
fn url(url: &Value) -> String {
    let raw = match url {
        Value::String(raw) => return raw.clone(),
        Value::Object(_) => url["raw"].as_str().unwrap_or_default(),
        _ => return String::new(),
    };

    let Some(variables) = url["variable"].as_array() else {
        return raw.to_owned();
    };

    let end = raw.find(['?', '#']).unwrap_or(raw.len());
    let (path, rest) = raw.split_at(end);
    let path = path
        .split('/')
        .map(|segment| {
            segment
                .strip_prefix(':')
                .and_then(|key| {
                    variables
                        .iter()
                        .find(|variable| variable["key"].as_str() == Some(key))
                })
                .map(|variable| text(variable.get("value")))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| segment.to_owned())
        })
        .collect::<Vec<_>>()
        .join("/");

    path + rest
}

fn body(body: &Value, headers: &mut Vec<(String, String)>) -> Option<Vec<u8>> {
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
            (!fields.is_empty()).then(|| url_encoded_form(&fields, headers))
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
            let variables = match &graphql["variables"] {
                Value::String(variables) if variables.trim().is_empty() => Value::Null,
                Value::String(variables) => serde_json::from_str(variables).unwrap_or(Value::Null),
                variables => variables.clone(),
            };
            let mut payload = serde_json::json!({ "query": text(graphql.get("query")) });
            if !variables.is_null() {
                payload["variables"] = variables;
            }
            set_content_type(headers, "application/json");

            Some(serde_json::to_string_pretty(&payload).ok()?.into_bytes())
        }
        _ => None,
    }
}

/// The item's own authorization, which `noauth` sets to none. Without one,
/// the item inherits its parent's.
fn own_auth(item: &Value) -> Option<&Value> {
    item.get("auth")
        .filter(|auth| !auth.is_null() && auth["type"].as_str() != Some("inherit"))
}

/// Applies an authorization as the headers or query parameters Postman sends.
fn authorize(
    auth: Option<&Value>,
    headers: &mut Vec<(String, String)>,
    query: &mut Vec<(String, String)>,
    scripts: &mut RequestScripts,
) {
    let Some(auth) = auth else { return };
    let kind = auth["type"].as_str().unwrap_or_default();
    let value = |key: &str| auth_value(auth, kind, key);
    let has_authorization = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));

    match kind {
        "bearer" if !has_authorization => {
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
                let script = format!(
                    "pm.request.headers.upsert({{key: \"Authorization\", value: \"Basic \" + pm.encoding.base64Encode(pm.variables.replaceIn({credentials}))}});"
                );
                scripts.pre_request = join_script(&script, &scripts.pre_request);
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
fn pairs(pairs: &Value) -> Vec<(String, String)> {
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

fn scripts(item: &Value) -> RequestScripts {
    let mut scripts = RequestScripts::default();

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

        let script = match event["listen"].as_str() {
            Some("prerequest") => &mut scripts.pre_request,
            Some("test") => &mut scripts.post_response,
            _ => continue,
        };
        *script = join_script(script, &source);
    }

    scripts
}

fn join_scripts(first: &RequestScripts, second: &RequestScripts) -> RequestScripts {
    RequestScripts {
        pre_request: join_script(&first.pre_request, &second.pre_request),
        post_response: join_script(&first.post_response, &second.post_response),
    }
}

fn join_script(first: &str, second: &str) -> String {
    [first, second]
        .into_iter()
        .filter(|script| !script.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
