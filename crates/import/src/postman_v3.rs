//! Postman Collection v3: the folder of YAML files that Postman writes for each
//! collection in a workspace connected to Git. Postman saves gRPC requests to
//! files only in this format.
//!
//! HTTP requests, folders and authorizations are rewritten in the shape of
//! v2.1, so they convert as they do in an exported collection.

use std::{ffi::OsStr, fs, path::Path};

use collection::{ImportedCollection, ImportedItem};
use request::{
    GrpcDefinition, GrpcRequest, GrpcScripts, GrpcSettings, Request, RequestScripts,
    WebSocketRequest,
};
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
    // A collection's definition is optional, so a folder without one is a
    // collection when it holds requests.
    let has_requests = fs::read_dir(directory)
        .map_err(|source| ImportError::Read {
            path: directory.to_owned(),
            source,
        })?
        .any(|entry| entry.is_ok_and(|entry| request_name(&entry.path()).is_some()));
    if !directory.join(DEFINITION).is_file() && !has_requests {
        return Err(ImportError::NotPostmanFolder);
    }

    let definition = read_definition(directory)?;
    let group = group(&definition);
    // Collection scripts become the collection's own scripts, so requests
    // inherit only its authorization.
    let inherited = Inherited {
        auth: postman::own_auth(&group),
        scripts: Scripts::default(),
    };
    let mut skipped = Vec::new();
    let items = items(
        directory,
        &inherited,
        &listed(&definition["auth"]),
        &mut skipped,
    )?;
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
/// `auths` are the authorizations its collection and folders list.
fn items(
    directory: &Path,
    inherited: &Inherited,
    auths: &[&Value],
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
            let document = read_definition(path)?;

            entries.push(Entry {
                name: clean_name(document["name"].as_str().or(Some(file_name)), "Untitled"),
                order: document["order"].as_f64(),
                path,
                document,
                is_folder: true,
            });
        } else if let Some(stem) = request_name(path) {
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
            let auths = [auths, &listed(&entry.document["auth"])].concat();

            converted.push(ImportedItem::Folder {
                items: items(entry.path, &inherited, &auths, skipped)?,
                name: entry.name,
            });
        } else {
            match request(&entry.document, entry.path, inherited, auths) {
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
fn request(
    request: &Value,
    path: &Path,
    inherited: &Inherited,
    auths: &[&Value],
) -> Option<Request> {
    let auth = auth(selected_auth(&request["auth"], auths));

    match request["$kind"].as_str()? {
        "http-request" => postman::request(&http_item(request, auth), inherited).map(Request::Http),
        "grpc-request" => Some(Request::Grpc(grpc(request, auth, path, inherited))),
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
fn http_item(request: &Value, auth: Value) -> Value {
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
            "auth": auth,
        },
    })
}

fn grpc(request: &Value, auth: Value, path: &Path, inherited: &Inherited) -> GrpcRequest {
    let mut metadata = postman::pairs(&entries(&request["metadata"]));
    // gRPC calls have no query parameters, so only authorizations sent as
    // metadata apply.
    let own = json!({ "auth": auth });
    postman::authorize(
        postman::own_auth(&own).or(inherited.auth),
        &mut metadata,
        &mut Vec::new(),
        &mut Scripts::default(),
    );

    let settings = &request["settings"];

    GrpcRequest {
        url: text(request.get("url")),
        tls: settings["secureConnection"].as_bool().unwrap_or_default(),
        method: method(request["methodPath"].as_str().unwrap_or_default()),
        message: text(request["message"].get("content")),
        metadata,
        definition: service_definition(&request["schema"], path.parent().unwrap_or(path)),
        settings: GrpcSettings {
            verify_certificates: settings["strictSSL"].as_bool(),
            server_name: text(settings.get("serverNameOverride")),
            // Postman shows them unless this is turned off.
            include_default_fields: settings["includeDefaultFields"].as_bool().unwrap_or(true),
            // Postman also counts in MiB and takes zero as any size.
            max_response_message_mb: settings["maxResponseMessageSize"].as_u64(),
            timeout_ms: None,
        },
        scripts: grpc_scripts(&request["scripts"]),
    }
}

/// A gRPC request's scripts, one for each hook Postman names.
fn grpc_scripts(scripts: &Value) -> GrpcScripts {
    let mut result = GrpcScripts::default();

    for script in scripts.as_array().into_iter().flatten() {
        let hook = match script["type"].as_str() {
            Some("beforeInvoke") => &mut result.before_invoke,
            Some("onMessage") => &mut result.on_message,
            Some("afterResponse") => &mut result.after_response,
            _ => continue,
        };
        *hook = text(script.get("code"));
    }

    result
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
fn service_definition(schema: &Value, directory: &Path) -> GrpcDefinition {
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

/// The authorizations a collection or folder lists.
fn listed(auth: &Value) -> Vec<&Value> {
    auth.as_array().into_iter().flatten().collect()
}

/// The authorization a request uses. One that inherits can name one of the
/// authorizations its collection and folders list by its `id`.
fn selected_auth<'a>(auth: &'a Value, auths: &[&'a Value]) -> &'a Value {
    // Postman writes credentials as a map and also reads a list of entries.
    let id = match &auth["credentials"] {
        Value::Array(credentials) => credentials
            .iter()
            .find(|credential| credential["key"] == "id")
            .map_or(&Value::Null, |credential| &credential["value"]),
        credentials => &credentials["id"],
    };
    if auth["type"] != "inherit" || id.is_null() {
        return auth;
    }

    auths
        .iter()
        .rev()
        .find(|listed| listed["id"] == *id)
        .copied()
        .unwrap_or(auth)
}

/// An authorization with its credentials under its type, as in v2.1.
fn auth(auth: &Value) -> Value {
    // Collections and folders list their authorizations; requests that do not
    // name one use the first.
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

/// A collection's or folder's definition, which is optional.
fn read_definition(directory: &Path) -> Result<Value, ImportError> {
    let path = directory.join(DEFINITION);

    if path.is_file() {
        yaml(&path)
    } else {
        Ok(Value::Null)
    }
}

/// A request file's name without its extension.
fn request_name(path: &Path) -> Option<&str> {
    file_name(path)?.strip_suffix(".request.yaml")
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
