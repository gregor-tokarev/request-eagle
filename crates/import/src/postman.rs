//! Postman Collection v2.0 and v2.1.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use collection::{ImportedCollection, ImportedItem};
use request::{
    Body, Field, FormPart, HttpRequest, HttpSettings, Method, RawLanguage, Request, RequestScripts,
};
use serde_json::Value;

use crate::{
    Import, ImportError,
    body::set_content_type,
    document::{clean_name, text},
};

pub(crate) fn convert(document: &Value) -> Result<Import, ImportError> {
    let mut skipped = Vec::new();
    // Collection scripts become the collection's own scripts, so requests
    // inherit only its authorization.
    let inherited = Inherited {
        auth: own_auth(document),
        scripts: Scripts::default(),
        behavior: Behavior::default().then(document),
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
    pub(crate) behavior: Behavior,
}

/// The parts of Postman's protocol profile behavior that request settings
/// keep. Collections and folders set it for the requests inside them, and
/// the closest explicit value wins.
#[derive(Clone, Copy, Default)]
pub(crate) struct Behavior {
    follow_redirects: Option<bool>,
    verify_certificates: Option<bool>,
}

impl Behavior {
    /// This behavior with the item's own values applied over it.
    pub(crate) fn then(self, item: &Value) -> Self {
        let own = &item["protocolProfileBehavior"];

        Self {
            follow_redirects: own["followRedirects"].as_bool().or(self.follow_redirects),
            verify_certificates: own["strictSSL"].as_bool().or(self.verify_certificates),
        }
    }
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
                    behavior: inherited.behavior.then(item),
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

    let mut headers = fields(&request["header"]);
    // The URL holds the parameters that are sent; those switched off stay
    // beside it.
    let mut query: Vec<_> = fields(&request["url"]["query"])
        .into_iter()
        .filter(|field| !field.enabled)
        .collect();
    let mut scripts = inherited.scripts.then(scripts(item));
    let body = body(&request["body"], &mut headers);
    // Postman applies authorization after the pre-request scripts, which may
    // set the credentials it uses.
    authorize(
        own_auth(request).or(inherited.auth),
        &mut headers,
        &mut query,
        &mut scripts,
    );

    // The request's Settings tab in Postman.
    let behavior = inherited.behavior.then(item);

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
            follow_redirects: behavior.follow_redirects,
            verify_certificates: behavior.verify_certificates,
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

fn body(body: &Value, headers: &mut Vec<Field>) -> Option<Body> {
    if body["disabled"].as_bool() == Some(true) {
        return None;
    }

    match body["mode"].as_str()? {
        "raw" => {
            let text = body["raw"]
                .as_str()
                .filter(|raw| !raw.is_empty())?
                .to_owned();
            // Other languages are sent as text of their own type.
            let language = match body["options"]["raw"]["language"].as_str() {
                Some("json") => RawLanguage::Json,
                Some("xml") => RawLanguage::Xml,
                Some("html") => {
                    set_content_type(headers, "text/html");
                    RawLanguage::Text
                }
                Some("javascript") => {
                    set_content_type(headers, "application/javascript");
                    RawLanguage::Text
                }
                _ => RawLanguage::Text,
            };

            Some(Body::Raw { language, text })
        }
        "urlencoded" => {
            let fields = pairs(&body["urlencoded"]);
            (!fields.is_empty()).then_some(Body::UrlEncoded { fields })
        }
        "formdata" => {
            let mut parts = Vec::new();
            let fields = body["formdata"].as_array().into_iter().flatten();
            for field in fields.filter(|field| field["disabled"].as_bool() != Some(true)) {
                let Some(name) = field["key"].as_str() else {
                    continue;
                };

                if field["type"].as_str() != Some("file") {
                    parts.push(FormPart {
                        name: name.to_owned(),
                        value: text(field.get("value")),
                        file: false,
                    });
                    continue;
                }

                // A field can send several files, or none chosen yet.
                let files = match &field["src"] {
                    Value::Array(files) => files.iter().filter_map(Value::as_str).collect(),
                    Value::String(file) => vec![file.as_str()],
                    _ => Vec::new(),
                };
                for file in if files.is_empty() { vec![""] } else { files } {
                    parts.push(FormPart {
                        name: name.to_owned(),
                        value: file.to_owned(),
                        file: true,
                    });
                }
            }

            (!parts.is_empty()).then_some(Body::Multipart { parts })
        }
        "file" => {
            let file = body["file"]["src"]
                .as_str()
                .filter(|file| !file.is_empty())?;
            Some(Body::Binary { file: file.into() })
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

            Some(Body::json(if variables.is_empty() {
                format!("{{\n  \"query\": {query}\n}}")
            } else {
                format!("{{\n  \"query\": {query},\n  \"variables\": {variables}\n}}")
            }))
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
    headers: &mut Vec<Field>,
    query: &mut Vec<Field>,
    scripts: &mut Scripts,
) {
    let Some(auth) = auth else { return };
    let kind = auth["type"].as_str().unwrap_or_default();
    let value = |key: &str| auth_value(auth, kind, key);
    let has_authorization =
        Field::enabled(headers).any(|(name, _)| name.eq_ignore_ascii_case("authorization"));

    match kind {
        // Postman sends no token when it is empty.
        "bearer" if !has_authorization && !value("token").is_empty() => {
            headers.push(Field::new(
                "Authorization",
                format!("Bearer {}", value("token")),
            ));
        }
        "oauth2" if !has_authorization && !value("accessToken").is_empty() => {
            headers.push(Field::new(
                "Authorization",
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
                headers.push(Field::new(
                    "Authorization",
                    format!("Basic {}", STANDARD.encode(credentials)),
                ));
            }
        }
        "apikey" => {
            let field = Field::new(value("key"), value("value"));
            if value("in") == "query" {
                query.push(field);
            } else {
                headers.push(field);
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

/// Header or metadata rows, including those switched off, with their
/// descriptions.
pub(crate) fn fields(fields: &Value) -> Vec<Field> {
    fields
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|field| {
            let description = &field["description"];

            Some(Field {
                key: field["key"].as_str()?.to_owned(),
                value: text(field.get("value")),
                enabled: field["disabled"].as_bool() != Some(true),
                // A description is text, or an object with its text as content.
                description: text(description.get("content").or(Some(description))),
            })
        })
        .collect()
}

/// Enabled key–value pairs, such as form fields.
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
