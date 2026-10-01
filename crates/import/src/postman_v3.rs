//! Postman Collection v3: the folder of YAML files that Postman writes for each
//! collection in a workspace connected to Git. Postman saves gRPC requests to
//! files only in this format.
//!
//! HTTP requests, folders and authorizations are rewritten in the shape of
//! v2.1, so they convert as they do in an exported collection.

use std::{ffi::OsStr, fs, path::Path};

use collection::{ImportedCollection, ImportedItem};
use request::{GrpcDefinition, GrpcRequest, Request, RequestScripts, WebSocketRequest};
use serde_json::{Map, Value, json};

use crate::{
    Import, ImportError,
    document::{clean_name, text},
    postman::{self, Inherited, Scripts},
};

/// A collection's or folder's name, order, variables, scripts and
/// authorization.
const DEFINITION: &str = ".resources/definition.yaml";

pub(crate) fn convert(directory: &Path) -> Result<Import, ImportError> {
    let definition = directory.join(DEFINITION);
    if !definition.is_file() {
        return Err(ImportError::NotPostmanFolder);
    }

    let definition = yaml(&definition)?;
    let group = group(&definition);
    // Collection scripts become the collection's own scripts, so requests
    // inherit only its authorization.
    let inherited = Inherited {
        auth: postman::own_auth(&group),
        scripts: Scripts::default(),
    };
    let mut skipped = Vec::new();
    let items = items(directory, &inherited, &mut skipped)?;
    let scripts = postman::scripts(&group);

    Ok(Import {
        collection: ImportedCollection {
            name: clean_name(
                definition["name"].as_str().or(file_name(directory)),
                "Postman Collection",
            ),
            variables: postman::pairs(&entries(&definition["variables"]))
                .into_iter()
                .collect(),
            scripts: RequestScripts {
                pre_request: postman::join_scripts(&scripts.pre_request),
                post_response: postman::join_scripts(&scripts.post_response),
            },
            items,
        },
        skipped,
    })
}

/// A folder or request file in a collection.
struct Entry<'a> {
    name: String,
    order: Option<f64>,
    path: &'a Path,
    document: Value,
    is_folder: bool,
}

/// The folders and requests in `directory`, in the order Postman shows them.
fn items(
    directory: &Path,
    inherited: &Inherited,
    skipped: &mut Vec<String>,
) -> Result<Vec<ImportedItem>, ImportError> {
    let read_error = |source| ImportError::Read {
        path: directory.to_owned(),
        source,
    };
    let paths = fs::read_dir(directory)
        .map_err(read_error)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(read_error)?;

    let mut entries = Vec::new();
    for path in &paths {
        let file_name = file_name(path).unwrap_or_default();
        // `.resources` holds definitions, examples and saved messages.
        if file_name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let definition = path.join(DEFINITION);
            let document = if definition.is_file() {
                yaml(&definition)?
            } else {
                Value::Null
            };

            entries.push(Entry {
                name: clean_name(document["name"].as_str().or(Some(file_name)), "Untitled"),
                order: document["order"].as_f64(),
                path,
                document,
                is_folder: true,
            });
        } else if let Some(stem) = file_name.strip_suffix(".request.yaml") {
            let document = yaml(path)?;

            entries.push(Entry {
                name: clean_name(document["name"].as_str().or(Some(stem)), "Untitled"),
                order: document["order"].as_f64(),
                path,
                document,
                is_folder: false,
            });
        }
    }

    // Items without an order come last.
    entries.sort_by(|a, b| {
        let order = |entry: &Entry| entry.order.unwrap_or(f64::INFINITY);
        order(a)
            .total_cmp(&order(b))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut converted = Vec::new();
    for entry in entries {
        if entry.is_folder {
            let group = group(&entry.document);
            let inherited = Inherited {
                auth: postman::own_auth(&group).or(inherited.auth),
                scripts: inherited.scripts.then(postman::scripts(&group)),
            };

            converted.push(ImportedItem::Folder {
                items: items(entry.path, &inherited, skipped)?,
                name: entry.name,
            });
        } else {
            match request(&entry.document, entry.path, inherited) {
                Some(request) => converted.push(ImportedItem::Request {
                    name: entry.name,
                    request,
                }),
                None => skipped.push(entry.name),
            }
        }
    }

    Ok(converted)
}

/// The request, or `None` when Request Eagle cannot send its protocol or
/// HTTP method.
fn request(request: &Value, path: &Path, inherited: &Inherited) -> Option<Request> {
    match request["$kind"].as_str()? {
        "http-request" => postman::request(&http_item(request), inherited).map(Request::Http),
        "grpc-request" => Some(Request::Grpc(grpc(request, path, inherited))),
        // Postman keeps a connection's saved messages in separate files.
        "websocket-request" => Some(Request::WebSocket(WebSocketRequest {
            url: text(request.get("url")),
            headers: postman::pairs(&entries(&request["headers"])),
            ..WebSocketRequest::default()
        })),
        _ => None,
    }
}

/// An HTTP request as a v2.1 item. Its URL already contains its query.
fn http_item(request: &Value) -> Value {
    json!({
        "event": events(&request["scripts"]),
        "request": {
            "method": request["method"].clone(),
            "url": {
                "raw": request["url"].clone(),
                "variable": entries(&request["pathVariables"]),
            },
            "header": entries(&request["headers"]),
            "body": body(&request["body"]),
            "auth": auth(&request["auth"]),
        },
    })
}

fn grpc(request: &Value, path: &Path, inherited: &Inherited) -> GrpcRequest {
    let mut metadata = postman::pairs(&entries(&request["metadata"]));
    // gRPC calls have no query parameters or scripts, so only authorizations
    // sent as metadata apply.
    let own = json!({ "auth": auth(&request["auth"]) });
    postman::authorize(
        postman::own_auth(&own).or(inherited.auth),
        &mut metadata,
        &mut Vec::new(),
        &mut Scripts::default(),
    );

    GrpcRequest {
        url: text(request.get("url")),
        tls: request["settings"]["secureConnection"]
            .as_bool()
            .unwrap_or_default(),
        method: method(request["methodPath"].as_str().unwrap_or_default()),
        message: text(request["message"].get("content")),
        metadata,
        definition: definition(&request["schema"], path.parent().unwrap_or(path)),
        ..GrpcRequest::default()
    }
}

/// The method as `package.Service/Method`; Postman writes
/// `package.Service.Method`.
fn method(path: &str) -> String {
    let path = path.trim().trim_start_matches('/');

    match path.rsplit_once('.') {
        Some((service, method)) if !path.contains('/') => format!("{service}/{method}"),
        _ => path.to_owned(),
    }
}

/// The `.proto` file Postman compiled, with a relative path resolved from the
/// request's folder so it still resolves after importing. Definitions stored
/// in Postman's cloud are not available, so the server is asked instead.
fn definition(schema: &Value, directory: &Path) -> GrpcDefinition {
    match schema["location"].as_str() {
        Some(location) if schema["source"] == "file" && !location.is_empty() => {
            GrpcDefinition::ProtoFile {
                path: directory.join(location),
                import_paths: Vec::new(),
            }
        }
        _ => GrpcDefinition::Reflection,
    }
}

/// A body in the shape of v2.1, where a text body is named by its language.
fn body(body: &Value) -> Value {
    let content = body["content"].clone();

    match body["type"].as_str() {
        Some("urlencoded") => json!({ "mode": "urlencoded", "urlencoded": entries(&content) }),
        Some("formdata") => json!({ "mode": "formdata", "formdata": entries(&content) }),
        Some("graphql") => json!({ "mode": "graphql", "graphql": content }),
        Some(language) => json!({
            "mode": "raw",
            "raw": content,
            "options": { "raw": { "language": language } },
        }),
        None => Value::Null,
    }
}

/// A folder's authorization and scripts in the shape of a v2.1 item.
fn group(definition: &Value) -> Value {
    json!({
        "auth": auth(&definition["auth"]),
        "event": events(&definition["scripts"]),
    })
}

/// An authorization with its credentials under its type, as in v2.1.
fn auth(auth: &Value) -> Value {
    // Collections and folders list their authorizations; the first is used.
    let auth = match auth {
        Value::Array(auths) => auths.first().unwrap_or(&Value::Null),
        auth => auth,
    };
    let Some(kind) = auth["type"].as_str() else {
        return Value::Null;
    };

    let mut converted = Map::new();
    converted.insert("type".into(), kind.into());
    converted.insert(kind.into(), auth["credentials"].clone());

    Value::Object(converted)
}

/// Scripts as v2.1 events. Collections and folders prefix a script's type
/// with the protocol it runs for; gRPC scripts have no counterpart.
fn events(scripts: &Value) -> Value {
    scripts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|script| {
            let listen = match script["type"].as_str()?.trim_start_matches("http:") {
                "beforeRequest" => "prerequest",
                "afterResponse" => "test",
                _ => return None,
            };

            Some(json!({ "listen": listen, "script": { "exec": script["code"].clone() } }))
        })
        .collect()
}

/// Key–value pairs as a v2.1 list. Postman writes them as a list of entries,
/// which may be disabled, or as a map.
fn entries(pairs: &Value) -> Value {
    match pairs {
        Value::Object(pairs) => pairs
            .iter()
            .map(|(key, value)| json!({ "key": key, "value": value }))
            .collect(),
        pairs => pairs.clone(),
    }
}

fn yaml(path: &Path) -> Result<Value, ImportError> {
    let source = fs::read_to_string(path).map_err(|source| ImportError::Read {
        path: path.to_owned(),
        source,
    })?;

    serde_saphyr::from_str(&source)
        .map_err(|error| ImportError::Syntax(format!("{}: {error}", path.display())))
}

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}
