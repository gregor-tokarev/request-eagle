use request::{ApiKeyLocation, Authentication, FormBody, HttpRequest, MultipartField};
use serde_json::{Map, Value};

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
    let profile = super::postman_profiles::inherit(&collection, &Map::new());
    let mut result = Vec::new();
    collect_items(items, &[], &auth, &profile, &mut result)?;

    if result.is_empty() {
        return Err("The Postman collection contains no requests.".into());
    }

    Ok(result)
}

fn collect_items(
    items: &[Value],
    folders: &[String],
    inherited_auth: &Authentication,
    inherited_profile: &Map<String, Value>,
    result: &mut Vec<ImportedRequest>,
) -> Result<(), String> {
    for (item, name) in items
        .iter()
        .zip(super::postman_folders::sibling_names(items))
    {
        reject_scripts(item).map_err(|error| format!("{name}: {error}"))?;
        let auth = authentication(item.get("auth"), inherited_auth)?;
        let profile = super::postman_profiles::inherit(item, inherited_profile);

        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let mut child_folders = folders.to_vec();
            child_folders.push(name.to_owned());
            collect_items(children, &child_folders, &auth, &profile, result)?;
        } else if let Some(value) = item.get("request") {
            let request =
                request(value, &auth, &profile).map_err(|error| format!("{name}: {error}"))?;
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

fn request(
    value: &Value,
    inherited_auth: &Authentication,
    profile: &Map<String, Value>,
) -> Result<HttpRequest, String> {
    let content_type_override = super::postman_profiles::truthy(
        profile
            .get("disabledSystemHeaders")
            .and_then(|headers| headers.get("content-type")),
    );

    if let Some(path) = value.as_str() {
        let request = HttpRequest {
            path: url_with_default_protocol(path)?,
            authentication: inherited_auth.clone(),
            ..Default::default()
        };
        super::postman_form_headers::validate_request(&request, content_type_override)?;
        super::postman_auth::validate(&request)?;
        super::postman_profiles::validate(profile, &request, None)?;

        return Ok(request);
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
                let (key, value) = header(line)?;
                // SDK string headers trim JavaScript whitespace at both ends.
                // Array descriptor values are copied verbatim instead.
                let value = value.trim_matches(|character: char| {
                    character == '\u{feff}'
                        || (character.is_whitespace() && character != '\u{0085}')
                });
                request.headers.push((key, value.into()));
            }
        }
        Some(Value::Array(headers)) => request.headers = pairs(headers)?,
        _ => return Err("Postman headers must be an array or text.".into()),
    }

    let mut header_spellings = std::collections::HashMap::new();

    for (name, _) in &request.headers {
        if name.contains("{{") {
            return Err("Postman header names must be resolved before importing. Define collection defaults or replace the header-name placeholders. Header values may still use environment variables.".into());
        }

        if let Some(previous) = header_spellings.insert(name.to_ascii_lowercase(), name)
            && previous != name
        {
            return Err(format!(
                "Postman header {name:?} appears with different capitalization. Import cannot preserve that precedence; use one spelling before importing."
            ));
        }
    }

    if let Some(body) = value.get("body").filter(|body| !body.is_null())
        && !disabled(body)
    {
        read_body(
            body,
            value.get("header"),
            content_type_override,
            &mut request,
        )?;
    }

    super::postman_form_headers::validate_request(&request, content_type_override)?;

    super::postman_auth::validate(&request)?;
    super::postman_profiles::validate(profile, &request, value.get("header"))?;

    Ok(request)
}

fn read_url(value: &Value, request: &mut HttpRequest) -> Result<(), String> {
    if value.get("auth").is_some_and(|auth| !auth.is_null()) {
        return Err("Postman structured URL authentication is not supported by import. Move the credentials to the request's Basic authentication helper before importing.".into());
    }

    request.path = if let Some(raw) = value.as_str() {
        url_with_default_protocol(raw)?
    } else if let Some(raw) = value.get("raw").and_then(Value::as_str)
        && !["protocol", "host", "port", "path", "query"]
            .iter()
            .any(|field| value.get(field).is_some())
    {
        // Accept raw-only objects as a convenience. In a structured URL, raw
        // is display metadata and must never supply missing target components.
        url_with_default_protocol(raw)?
    } else {
        let protocol = value
            .get("protocol")
            .and_then(Value::as_str)
            .filter(|protocol| !protocol.is_empty())
            .map(|protocol| format!("{protocol}://"))
            .unwrap_or_default();
        let host = joined(value.get("host").filter(|host| !host.is_null()), ".")?;
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
            return Err("The structured Postman URL has no host. Supply a host or use a URL string before importing.".into());
        }

        // A base-URL variable in host may supply its own protocol. Infer HTTP
        // only after assembling the actual structured URL.
        url_with_default_protocol(&format!("{protocol}{host}{port}/{path}"))?
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
        // URL path variables use the last definition, even when disabled.
        // Empty definitions mask earlier values instead of falling back.
        let variables = variables
            .iter()
            .map(|variable| {
                if let Some(kind) = variable.get("type")
                    && (!kind
                        .as_str()
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("string"))
                        || !variable.get("value").is_some_and(Value::is_string))
                {
                    return Err("Postman path variables with a declared type must use string type and a text value before importing.".into());
                }

                pair(variable)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let suffix_start = request.path.find(['?', '#']).unwrap_or(request.path.len());
        let (path, suffix) = request.path.split_at(suffix_start);
        let resolved_path = path
            .split('/')
            .map(|segment| {
                let Some(variable) = segment.strip_prefix(':') else {
                    return segment.to_owned();
                };
                let name = variable.split('.').next().unwrap_or_default();

                match variables.iter().rev().find(|(key, _)| key == name) {
                    Some((_, value)) if !value.is_empty() => {
                        format!("{value}{}", &variable[name.len()..])
                    }
                    _ => segment.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        request.path = format!("{resolved_path}{suffix}");
    }

    reject_url_controls(&request.path)
}

fn url_with_default_protocol(value: &str) -> Result<String, String> {
    reject_url_controls(value)?;
    let value = value.trim();

    if value.is_empty() {
        return Err("The request URL is empty.".into());
    }

    let protocol = value.split_once("://").filter(|(protocol, _)| {
        protocol.starts_with(|character: char| character.is_ascii_alphabetic())
            && protocol
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "+-.".contains(character))
    });

    if let Some((protocol, _)) = protocol
        && !protocol.eq_ignore_ascii_case("http")
        && !protocol.eq_ignore_ascii_case("https")
    {
        return Err(format!(
            "Postman protocol {protocol:?} is not supported. Import an HTTP or HTTPS URL."
        ));
    }

    let has_protocol_template = value
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|prefix| prefix.contains("{{"));

    // An environment variable before the path may supply the protocol or whole
    // URL. Collection defaults have already been resolved before parsing.
    if protocol.is_some() || has_protocol_template {
        Ok(value.to_owned())
    } else if value.starts_with('/') {
        Err("A scheme-less Postman URL must start with a hostname.".into())
    } else {
        Ok(format!("http://{value}"))
    }
}

fn reject_url_controls(value: &str) -> Result<(), String> {
    // Postman escapes these bytes; the executor's URL parser discards them.
    // Check before trimming and again after substituting URL path variables.
    if value.contains(['\t', '\r', '\n']) {
        return Err("Postman URLs containing literal TAB, CR, or LF are not supported by import. Percent-encode these characters before importing.".into());
    }

    Ok(())
}

fn read_query(query: &[Value], request: &mut HttpRequest) -> Result<(), String> {
    let mut fields = Vec::new();
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
        fields.push((name, value));
    }

    if templated {
        if has_flags {
            return Err("Postman templated queries with valueless flags are not supported.".into());
        }

        // Decode literal spans before substitution so environment values are
        // encoded as query fields. Placeholder names must remain untouched.
        request.query = Some(
            fields
                .into_iter()
                .map(|(name, value)| {
                    Ok((
                        decode_templated_query_component(name)?,
                        decode_templated_query_component(value.unwrap_or_default())?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        );
    } else if !fields.is_empty() {
        let encoded = fields
            .into_iter()
            .map(|(name, value)| {
                let name = encode_query_component(name, true);
                match value {
                    Some(value) => format!("{name}={}", encode_query_component(value, false)),
                    None => name,
                }
            })
            .collect::<Vec<_>>();
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
            "Postman query fields containing percent-encoded braces are not supported in editable query fields.".into(),
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
        .map_err(|_| "Editable Postman query fields must contain UTF-8 text.".into())
}

fn decode_templated_query_component(value: &str) -> Result<String, String> {
    let mut decoded = String::new();
    let mut rest = value;

    while let Some(start) = rest.find("{{") {
        decoded.push_str(&decode_query_component(&rest[..start])?);
        let Some(end) = rest[start + 2..].find("}}") else {
            decoded.push_str(&rest[start..]);
            return Ok(decoded);
        };
        let end = start + 2 + end + 2;
        decoded.push_str(&rest[start..end]);
        rest = &rest[end..];
    }

    decoded.push_str(&decode_query_component(rest)?);
    Ok(decoded)
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

fn read_body(
    body: &Value,
    source_headers: Option<&Value>,
    content_type_override: bool,
    request: &mut HttpRequest,
) -> Result<(), String> {
    let Some(mode) = body.get("mode") else {
        // Retained editor text is inactive until Postman selects a body mode.
        return Ok(());
    };
    let mode = mode.as_str().ok_or("The Postman body mode must be text.")?;

    match mode {
        "raw" => {
            let raw = match body.get("raw") {
                None | Some(Value::Null) => "",
                Some(Value::String(raw)) => raw,
                _ => return Err("The raw Postman body must be text.".into()),
            };

            if raw.is_empty() {
                return Ok(());
            }

            if content_type_override {
                super::postman_form_headers::validate_raw(source_headers, &request.headers)?;
            }

            let language = body
                .pointer("/options/raw/language")
                .and_then(Value::as_str);
            let content_type = match language {
                Some("json") => "application/json",
                Some("javascript") => "application/javascript",
                Some("xml") => "application/xml",
                Some("html") => "text/html",
                _ => "text/plain",
            };

            add_content_type(request, content_type);

            let is_json = strips_json_comments(raw, language, source_headers, &request.headers)?;
            request.body = Some(if is_json {
                super::json_comments::strip(raw).into_bytes()
            } else {
                raw.as_bytes().to_vec()
            });
        }
        "urlencoded" => {
            let values = body
                .get("urlencoded")
                .and_then(Value::as_array)
                .ok_or("The urlencoded body must contain a field array.")?;
            let fields = pairs(values)?;

            if !fields.is_empty() {
                request.form = Some(FormBody::UrlEncoded(fields));
            }
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

                if field.get("fileName").is_some_and(|name| !name.is_null()) {
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

            if !fields.is_empty() {
                request.form = Some(FormBody::Multipart(fields));
            }
        }
        mode => {
            return Err(format!(
                "Postman body mode {mode:?} is not supported by import. Convert it to raw or urlencoded first."
            ));
        }
    }

    Ok(())
}

fn strips_json_comments(
    raw: &str,
    language: Option<&str>,
    source_headers: Option<&Value>,
    headers: &[(String, String)],
) -> Result<bool, String> {
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        return Ok(language == "json");
    }

    let unsupported = "Commented raw Postman bodies with a dynamic Content-Type are not supported by import. Set a literal Content-Type or raw language before importing.";
    let dynamic_name = match source_headers.and_then(Value::as_array) {
        Some(headers) => headers.iter().any(|header| {
            header
                .get("key")
                .and_then(Value::as_str)
                .is_some_and(|name| name.contains("{{"))
        }),
        None => headers.iter().any(|(name, _)| name.contains("{{")),
    };

    if dynamic_name && super::json_comments::strip(raw) != raw {
        return Err(unsupported.into());
    }

    let matches = |value: &str| {
        if value.contains("{{") && super::json_comments::strip(raw) != raw {
            Err(unsupported.to_owned())
        } else {
            Ok(super::json_comments::is_json_content_type(value))
        }
    };

    if let Some(source_headers) = source_headers.and_then(Value::as_array) {
        let content_types = source_headers
            .iter()
            .filter(|header| {
                header
                    .get("key")
                    .and_then(Value::as_str)
                    .is_some_and(|key| key.eq_ignore_ascii_case("content-type"))
            })
            .collect::<Vec<_>>();

        if content_types.is_empty() {
            return Ok(false);
        }

        // Match Postman's presend selection, including its disabled-header
        // behavior. Disabled descriptors still influence body preparation even
        // though they are excluded from the outgoing headers above.
        let selected = if content_types.len() == 1 {
            content_types.first().copied()
        } else {
            content_types.into_iter().find(|header| !disabled(header))
        };

        return selected
            .and_then(|header| header.get("value"))
            .and_then(Value::as_str)
            .map_or(Ok(true), matches);
    }

    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map_or(Ok(false), |(_, value)| matches(value))
}

fn pairs(values: &[Value]) -> Result<Vec<(String, String)>, String> {
    values
        .iter()
        .filter(|value| !disabled(value))
        .map(pair)
        .collect()
}

fn pair(value: &Value) -> Result<(String, String), String> {
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
    let attributes = match value.get(kind) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(attributes)) => attributes
            .iter()
            .map(|(name, value)| (name.as_str(), Some(value)))
            .collect(),
        Some(Value::Array(attributes)) => {
            let mut assigned = std::collections::HashSet::new();

            attributes
                .iter()
                .map(|attribute| {
                    let name = attribute
                        .get("key")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!("Postman {kind} authentication attribute keys must be text.")
                        })?;
                    let value = attribute.get("value");

                    if let Some(value_type) = attribute.get("type") {
                        if value_type.as_str() != Some("string") {
                            return Err(format!("Postman {kind} authentication attribute {name:?} must use string type before importing."));
                        }

                        if value.is_none() && !assigned.contains(name) {
                            return Err(format!("Postman {kind} authentication attribute {name:?} with string type needs an explicit value before importing."));
                        }
                    }

                    if value.is_some_and(Value::is_string) {
                        assigned.insert(name);
                    }

                    Ok((name, value))
                })
                .collect::<Result<Vec<_>, String>>()?
        }
        _ => {
            return Err(format!(
                "Postman {kind} authentication attributes must be an object or an array."
            ));
        }
    };

    for (name, value) in &attributes {
        if !matches!(value, None | Some(Value::String(_))) {
            return Err(format!(
                "Postman {kind} authentication attribute {name:?} must be text."
            ));
        }
    }

    let attribute = |name: &str| {
        attributes
            .iter()
            .rev()
            .find(|(key, value)| *key == name && value.is_some())
            .and_then(|(_, value)| *value)
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
        "bearer" => {
            let token = attribute("token");

            if token.is_empty() {
                Authentication::None
            } else {
                Authentication::Bearer { token }
            }
        }
        "apikey" => {
            let name = attribute("key");
            let value = attribute("value");

            if name.is_empty() && value.is_empty() {
                Authentication::None
            } else {
                let location = match attribute("in").as_str() {
                    "header" | "" => ApiKeyLocation::Header,
                    "query" => ApiKeyLocation::Query,
                    location => return Err(format!("Unsupported API key location {location:?}.")),
                };
                let (name, value) = if location == ApiKeyLocation::Query {
                    // SDK query credentials retain existing escapes and '+'.
                    // The editor stores decoded fields, then encodes at Send.
                    (
                        decode_templated_query_component(&name)?,
                        decode_templated_query_component(&value)?,
                    )
                } else {
                    (name, value)
                };

                Authentication::ApiKey {
                    name,
                    value,
                    location,
                }
            }
        }
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
