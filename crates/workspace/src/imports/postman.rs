use request::{ApiKeyLocation, Authentication, HttpRequest};
use serde_json::Value;

use super::parser::{ImportedRequest, add_content_type, header, method};

pub(super) fn parse(input: &str) -> Result<Vec<ImportedRequest>, String> {
    let mut collection: Value = serde_json::from_str(input)
        .map_err(|error| format!("Invalid Postman collection JSON: {error}"))?;
    super::variables::resolve_collection_defaults(&mut collection)?;
    let items = collection
        .get("item")
        .and_then(Value::as_array)
        .ok_or("Expected a Postman v2 collection with an item array.")?;

    if let Some(schema) = collection.pointer("/info/schema").and_then(Value::as_str)
        && !schema.contains("/v2.")
    {
        return Err("Only Postman v2 collections are supported.".into());
    }

    reject_scripts(&collection)?;

    let auth = authentication(collection.get("auth"), &Authentication::None)?;
    let mut result = Vec::new();
    collect_items(items, &[], &auth, &mut result)?;

    if result.is_empty() {
        return Err("The Postman collection contains no requests.".into());
    }

    Ok(result)
}

fn collect_items(
    items: &[Value],
    folders: &[String],
    inherited_auth: &Authentication,
    result: &mut Vec<ImportedRequest>,
) -> Result<(), String> {
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Imported request");
        reject_scripts(item).map_err(|error| format!("{name}: {error}"))?;
        let auth = authentication(item.get("auth"), inherited_auth)?;

        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let mut child_folders = folders.to_vec();
            child_folders.push(name.to_owned());
            collect_items(children, &child_folders, &auth, result)?;
        } else if let Some(value) = item.get("request") {
            let request = request(value, &auth).map_err(|error| format!("{name}: {error}"))?;
            result.push(ImportedRequest {
                name: name.into(),
                folders: folders.to_vec(),
                request,
            });
        } else {
            return Err(format!(
                "Postman item {name:?} has neither requests nor a folder."
            ));
        }
    }

    Ok(())
}

fn request(value: &Value, inherited_auth: &Authentication) -> Result<HttpRequest, String> {
    if let Some(path) = value.as_str() {
        return Ok(HttpRequest {
            path: path.into(),
            authentication: inherited_auth.clone(),
            ..Default::default()
        });
    }

    if !value.is_object() {
        return Err("The request must be a URL string or a request object.".into());
    }

    reject_scripts(value)?;

    let mut request = HttpRequest {
        method: method(value.get("method").and_then(Value::as_str).unwrap_or("GET"))?,
        authentication: authentication(value.get("auth"), inherited_auth)?,
        ..Default::default()
    };

    read_url(
        value.get("url").ok_or("The request has no URL.")?,
        &mut request,
    )?;

    match value.get("header") {
        None | Some(Value::Null) => {}
        Some(Value::String(headers)) => {
            for line in headers.lines().filter(|line| !line.trim().is_empty()) {
                request.headers.push(header(line)?);
            }
        }
        Some(Value::Array(headers)) => request.headers = pairs(headers)?,
        _ => return Err("Postman headers must be an array or text.".into()),
    }

    if let Some(body) = value.get("body").filter(|body| !body.is_null())
        && !disabled(body)
    {
        read_body(body, &mut request)?;
    }

    Ok(request)
}

fn read_url(value: &Value, request: &mut HttpRequest) -> Result<(), String> {
    request.path = if let Some(raw) = value.as_str() {
        raw.into()
    } else if let Some(raw) = value.get("raw").and_then(Value::as_str) {
        raw.into()
    } else {
        let protocol = value
            .get("protocol")
            .and_then(Value::as_str)
            .unwrap_or("https");
        let host = joined(value.get("host"), ".")?;
        let path = joined(value.get("path"), "/")?;
        let port = value
            .get("port")
            .and_then(Value::as_str)
            .map(|port| format!(":{port}"))
            .unwrap_or_default();

        if host.is_empty() {
            return Err("The Postman URL has no host or raw URL.".into());
        }

        format!("{protocol}://{host}{port}/{path}")
    };

    if request.path.is_empty() {
        return Err("The request URL is empty.".into());
    }

    if let Some(query) = value.get("query").and_then(Value::as_array) {
        // The structured query is authoritative and includes disabled pairs.
        // Strip its raw copy so execution appends each enabled pair only once.
        let path = request.path.split('#').next().unwrap_or_default();
        request.path = path.split('?').next().unwrap_or_default().to_owned();
        request.query = Some(pairs(query)?);
    }

    if let Some(variables) = value.get("variable").and_then(Value::as_array) {
        let variables = pairs(variables)?;
        request.path = request
            .path
            .split('/')
            .map(|segment| {
                segment
                    .strip_prefix(':')
                    .and_then(|name| variables.iter().find(|(key, _)| key == name))
                    .map(|(_, value)| value.as_str())
                    .unwrap_or(segment)
            })
            .collect::<Vec<_>>()
            .join("/");
    }

    Ok(())
}

fn joined(value: Option<&Value>, separator: &str) -> Result<String, String> {
    match value {
        None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| {
                part.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Postman URL parts must be strings.".into())
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|parts| parts.join(separator)),
        _ => Err("Postman URL parts must be strings or arrays.".into()),
    }
}

fn read_body(body: &Value, request: &mut HttpRequest) -> Result<(), String> {
    match body.get("mode").and_then(Value::as_str).unwrap_or("raw") {
        "raw" => {
            let raw = match body.get("raw") {
                None | Some(Value::Null) => "",
                Some(Value::String(raw)) => raw,
                _ => return Err("The raw Postman body must be text.".into()),
            };
            request.body = Some(raw.as_bytes().to_vec());

            if let Some(language) = body
                .pointer("/options/raw/language")
                .and_then(Value::as_str)
            {
                let content_type = match language {
                    "json" => "application/json",
                    "javascript" => "application/javascript",
                    "xml" => "application/xml",
                    "html" => "text/html",
                    _ => "text/plain",
                };
                add_content_type(request, content_type);
            }
        }
        "urlencoded" => {
            let values = body
                .get("urlencoded")
                .and_then(Value::as_array)
                .ok_or("The urlencoded body must contain a field array.")?;
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs(values)?)
                .finish();
            request.body = Some(encoded.into_bytes());
            add_content_type(request, "application/x-www-form-urlencoded");
        }
        mode => {
            return Err(format!(
                "Postman body mode {mode:?} is not supported by import. Convert it to raw or urlencoded first."
            ));
        }
    }

    Ok(())
}

fn pairs(values: &[Value]) -> Result<Vec<(String, String)>, String> {
    values
        .iter()
        .filter(|value| !disabled(value))
        .map(|value| {
            let key = value
                .get("key")
                .and_then(Value::as_str)
                .ok_or("A Postman field is missing its key.")?;
            let value = match value.get("value") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(value)) => value.clone(),
                _ => return Err("Postman field values must be strings.".into()),
            };

            Ok((key.into(), value))
        })
        .collect()
}

fn authentication(
    value: Option<&Value>,
    inherited: &Authentication,
) -> Result<Authentication, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(inherited.clone());
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Postman authentication has no type.")?;
    let attributes = value
        .get(kind)
        .and_then(Value::as_array)
        .map(|pairs| pairs.as_slice())
        .unwrap_or_default();
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|attribute| attribute.get("key").and_then(Value::as_str) == Some(name))
            .and_then(|attribute| attribute.get("value"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };

    Ok(match kind {
        "noauth" => Authentication::None,
        "basic" => Authentication::Basic {
            username: attribute("username"),
            password: attribute("password"),
        },
        "bearer" => Authentication::Bearer {
            token: attribute("token"),
        },
        "apikey" => Authentication::ApiKey {
            name: attribute("key"),
            value: attribute("value"),
            location: match attribute("in").as_str() {
                "header" | "" => ApiKeyLocation::Header,
                "query" => ApiKeyLocation::Query,
                location => return Err(format!("Unsupported API key location {location:?}.")),
            },
        },
        kind => {
            return Err(format!(
                "Postman authentication type {kind:?} is not supported."
            ));
        }
    })
}

fn disabled(value: &Value) -> bool {
    value
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn reject_scripts(value: &Value) -> Result<(), String> {
    if value
        .get("event")
        .and_then(Value::as_array)
        .is_some_and(|events| {
            events.iter().any(|event| {
                !disabled(event)
                    && event.get("script").is_some_and(|script| {
                        script.get("src").is_some()
                            || match script.get("exec") {
                                Some(Value::Array(lines)) => lines.iter().any(|line| {
                                    line.as_str().is_some_and(|line| !line.trim().is_empty())
                                }),
                                Some(Value::String(script)) => !script.trim().is_empty(),
                                _ => false,
                            }
                    })
            })
        })
    {
        return Err("Postman scripts cannot be imported. Remove the scripts before importing this collection.".into());
    }

    Ok(())
}
