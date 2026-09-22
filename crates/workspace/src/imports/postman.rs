use request::{ApiKeyLocation, Authentication, FormBody, HttpRequest, MultipartField};
use serde_json::Value;

use super::parser::{ImportedRequest, add_content_type, header, method, upload_path};

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
        let path = match value.get("path") {
            // Postman string paths include their leading separator; arrays hold
            // segments, where an empty first segment intentionally means '//'.
            Some(Value::String(path)) => path.strip_prefix('/').unwrap_or(path).to_owned(),
            path => joined(path, "/")?,
        };
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
        // Postman's structured query retains percent escapes and valueless flags.
        // Keep static queries in the URL instead of encoding those escapes twice.
        let path = request.path.split('#').next().unwrap_or_default();
        request.path = path.split('?').next().unwrap_or_default().to_owned();
        read_query(query, request)?;
    }

    if let Some(variables) = value.get("variable").and_then(Value::as_array) {
        let variables = pairs(variables)?;
        let suffix_start = request.path.find(['?', '#']).unwrap_or(request.path.len());
        let (path, suffix) = request.path.split_at(suffix_start);
        let resolved_path = path
            .split('/')
            .map(|segment| {
                let Some(variable) = segment.strip_prefix(':') else {
                    return segment.to_owned();
                };
                let name = variable.split('.').next().unwrap_or_default();

                match variables
                    .iter()
                    .find(|(key, value)| key == name && !value.is_empty())
                {
                    Some((_, value)) => format!("{value}{}", &variable[name.len()..]),
                    None => segment.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        request.path = format!("{resolved_path}{suffix}");
    }

    Ok(())
}

fn read_query(query: &[Value], request: &mut HttpRequest) -> Result<(), String> {
    let mut encoded = Vec::new();
    let mut templated = false;
    let mut has_flags = false;

    for field in query.iter().filter(|field| !disabled(field)) {
        let name = match field.get("key") {
            None | Some(Value::Null) => "",
            Some(Value::String(name)) => name,
            _ => return Err("Postman query names must be text.".into()),
        };
        let value = match field.get("value") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value.as_str()),
            _ => return Err("Postman query values must be text.".into()),
        };

        templated |= name.contains("{{") || value.is_some_and(|value| value.contains("{{"));
        has_flags |= value.is_none();
        let name = encode_query_component(name, true);
        encoded.push(match value {
            Some(value) => format!("{name}={}", encode_query_component(value, false)),
            None => name,
        });
    }

    if templated {
        if has_flags {
            return Err("Postman templated queries with valueless flags are not supported.".into());
        }

        // Decode before runtime substitution so values supplied by the active
        // environment are encoded as query values, including '&' and '='.
        request.query = Some(
            encoded
                .into_iter()
                .map(|field| {
                    let (name, value) = field.split_once('=').unwrap_or((&field, ""));
                    Ok((
                        decode_query_component(name)?,
                        decode_query_component(value)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        );
    } else if !encoded.is_empty() {
        request.path.push('?');
        request.path.push_str(&encoded.join("&"));
    }

    Ok(())
}

fn encode_query_component(value: &str, key: bool) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        if byte <= 0x20
            || byte >= 0x7f
            || matches!(byte, b'"' | b'#' | b'\'' | b'<' | b'>' | b'&')
            || (key && byte == b'=')
        {
            use std::fmt::Write as _;

            write!(encoded, "%{byte:02X}").unwrap();
        } else {
            encoded.push(byte as char);
        }
    }

    encoded
}

fn decode_query_component(value: &str) -> Result<String, String> {
    let lower = value.to_ascii_lowercase();

    if lower.contains("%7b") || lower.contains("%7d") {
        return Err(
            "Postman templated queries containing percent-encoded braces are not supported.".into(),
        );
    }

    let mut decoded = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(if bytes[index] == b'+' {
                b' '
            } else {
                bytes[index]
            });
            index += 1;
        }
    }

    String::from_utf8(decoded)
        .map_err(|_| "Templated Postman query fields must contain UTF-8 text.".into())
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
            request.form = Some(FormBody::UrlEncoded(pairs(values)?));
        }
        "formdata" => {
            let values = body
                .get("formdata")
                .and_then(Value::as_array)
                .ok_or("The multipart body must contain a field array.")?;
            let mut fields = Vec::new();

            for field in values.iter().filter(|field| !disabled(field)) {
                let name = field
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or("A Postman multipart field is missing its name.")?;

                if field
                    .get("contentType")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
                {
                    return Err(
                        "Custom Postman multipart content types are not supported by import."
                            .into(),
                    );
                }

                if field
                    .get("fileName")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty())
                {
                    return Err(
                        "Custom Postman upload filenames are not supported by import.".into(),
                    );
                }

                match field.get("type").and_then(Value::as_str).unwrap_or("text") {
                    "text" => fields.push(MultipartField::Text {
                        name: name.into(),
                        value: field
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                    }),
                    "file" => {
                        let paths = match field.get("src") {
                            Some(Value::String(path)) => vec![path.as_str()],
                            Some(Value::Array(paths)) => paths
                                .iter()
                                .map(|path| {
                                    path.as_str().ok_or("Postman upload paths must be strings.")
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                            _ => {
                                return Err(
                                    "A Postman upload is missing its local file path.".into()
                                );
                            }
                        };

                        if paths.is_empty() {
                            return Err("A Postman upload has no selected files.".into());
                        }

                        for path in paths {
                            fields.push(MultipartField::File {
                                name: name.into(),
                                path: upload_path(path)?,
                            });
                        }
                    }
                    kind => {
                        return Err(format!(
                            "Unsupported Postman multipart field type {kind:?}."
                        ));
                    }
                }
            }

            request.form = Some(FormBody::Multipart(fields));
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
