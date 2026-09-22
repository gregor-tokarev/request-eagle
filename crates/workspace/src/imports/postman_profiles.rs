use request::{ApiKeyLocation, Authentication, HttpRequest, Method};
use serde_json::{Map, Value};

pub(super) fn inherit(item: &Value, inherited: &Map<String, Value>) -> Map<String, Value> {
    let mut profile = inherited.clone();

    if let Some(overrides) = item
        .get("protocolProfileBehavior")
        .and_then(Value::as_object)
    {
        // Postman replaces each top-level property, including the whole
        // disabledSystemHeaders map, instead of recursively merging maps.
        profile.extend(overrides.clone());
    }

    profile
}

pub(super) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

pub(super) fn validate(
    profile: &Map<String, Value>,
    request: &HttpRequest,
    source_headers: Option<&Value>,
) -> Result<(), String> {
    if profile.contains_key("followRedirects") && !truthy(profile.get("followRedirects")) {
        return unsupported("followRedirects");
    }

    for key in [
        "followOriginalHttpMethod",
        "followAuthorizationHeader",
        "removeRefererHeaderOnRedirect",
        "disableUrlEncoding",
    ] {
        if truthy(profile.get(key)) {
            return unsupported(key);
        }
    }

    // These are per-request choices in Postman. Imported requests use the
    // application's transport preferences and cannot retain these overrides.
    for key in ["maxRedirects", "strictSSL", "protocolVersion"] {
        if profile.contains_key(key) {
            return unsupported(key);
        }
    }

    if let Some(value) = profile.get("insecureHTTPParser")
        && value != &Value::Bool(false)
    {
        return unsupported("insecureHTTPParser");
    }

    if let Some(protocols) = profile
        .get("tlsDisabledProtocols")
        .and_then(Value::as_array)
    {
        for protocol in protocols {
            let Some(protocol) = protocol.as_str() else {
                return unsupported("tlsDisabledProtocols");
            };

            // Runtime looks up SSL_OP_NO_<name>. Old protocol versions and
            // compression are already disabled by both default TLS clients.
            if matches!(
                protocol,
                "TLSv1_2"
                    | "TLSv1_3"
                    | "ENCRYPT_THEN_MAC"
                    | "QUERY_MTU"
                    | "RENEGOTIATION"
                    | "SESSION_RESUMPTION_ON_RENEGOTIATION"
                    | "TICKET"
            ) {
                return unsupported("tlsDisabledProtocols");
            }
        }
    }

    if let Some(ciphers) = profile.get("tlsCipherSelection").and_then(Value::as_array) {
        let empty = ciphers.is_empty()
            || (ciphers.len() == 1 && (ciphers[0].is_null() || ciphers[0].as_str() == Some("")));

        if !empty {
            return unsupported("tlsCipherSelection");
        }
    }

    // disableBodyPruning cannot affect an accepted GET/HEAD body: the importer
    // rejects all such bodies before returning requests to the editor.
    // Enabling disableCookies matches the client's lack of a cookie jar. The TLS
    // prefer-server-ciphers option is inert for a client connection.
    validate_system_headers(
        profile.get("disabledSystemHeaders"),
        request,
        source_headers,
    )
}

fn validate_system_headers(
    disabled: Option<&Value>,
    request: &HttpRequest,
    source_headers: Option<&Value>,
) -> Result<(), String> {
    let is_disabled = |key: &str| truthy(disabled.and_then(|headers| headers.get(key)));
    let api_header = match &request.authentication {
        Authentication::ApiKey {
            name,
            location: ApiKeyLocation::Header,
            ..
        } => Some(name.as_str()),
        _ => None,
    };
    let suppressed_system_headers = [
        "content-type",
        "connection",
        "host",
        "accept-encoding",
        "content-length",
    ];

    if api_header.is_some_and(|name| name.contains("{{"))
        && suppressed_system_headers.iter().any(|key| is_disabled(key))
    {
        return unsupported("disabledSystemHeaders with a variable API-key header name");
    }

    // Postman signs before applying header suppression. Its API-key helper
    // replaces the matching source descriptor with a system-owned descriptor.
    let header = |key| {
        if api_header.is_some_and(|name| name.eq_ignore_ascii_case(key)) {
            Some(true)
        } else {
            last_header(source_headers, &request.headers, key)
        }
    };
    let system = |key| header(key) == Some(true);
    let missing = |key| header(key).is_none();

    for key in suppressed_system_headers {
        if is_disabled(key) && system(key) {
            return unsupported(&format!("disabledSystemHeaders.{key}"));
        }
    }

    if is_disabled("host") && missing("host") {
        return unsupported("disabledSystemHeaders.host");
    }

    if is_disabled("accept") && missing("accept") {
        return unsupported("disabledSystemHeaders.accept");
    }

    if is_disabled("accept-encoding") && missing("accept-encoding") && missing("range") {
        return unsupported("disabledSystemHeaders.accept-encoding");
    }

    if is_disabled("content-length") {
        let generated_length = request.form.is_some()
            || request.body.as_ref().is_some_and(|body| !body.is_empty())
            || matches!(request.method, Method::Post | Method::Put | Method::Patch);

        if system("transfer-encoding")
            || (missing("content-length")
                && (missing("transfer-encoding") || request.form.is_some())
                && generated_length)
        {
            return unsupported("disabledSystemHeaders.content-length");
        }
    }

    Ok(())
}

// None means absent; Some(true) means the last enabled descriptor is system-owned.
pub(super) fn last_header(
    source: Option<&Value>,
    headers: &[(String, String)],
    key: &str,
) -> Option<bool> {
    match source.and_then(Value::as_array) {
        Some(headers) => headers
            .iter()
            .rev()
            .find(|header| {
                !truthy(header.get("disabled"))
                    && header
                        .get("key")
                        .and_then(Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case(key))
            })
            .map(|header| truthy(header.get("system"))),
        None => headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(key))
            .then_some(false),
    }
}

fn unsupported(key: &str) -> Result<(), String> {
    Err(format!(
        "Postman protocol profile setting {key} is not supported by import. Remove the override before importing."
    ))
}
