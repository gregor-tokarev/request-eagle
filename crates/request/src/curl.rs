//! Requests written as cURL commands, in the form of Postman's cURL code
//! snippets: long option names, single quotes and one option per line.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use environment::VariableResolver;
use url::{Url, form_urlencoded::byte_serialize};

use crate::{
    Auth, AuthLocation, Body, CookieJar, Field, HttpRequest, HttpVersion, Method,
    RequestPreferences,
};

impl HttpRequest {
    /// The cURL command that sends this request as Request Eagle does, with
    /// the headers it adds that cURL would send differently or not at all,
    /// and the redirects, timeout, certificate checks and HTTP version of the
    /// request's settings or, where they have none, of `preferences`.
    /// `{{variables}}` that `values` defines are filled in as sending fills
    /// them, including `:name` path variables; others, and generated ones
    /// such as `{{$guid}}`, stay as written. The cookies that `cookies` would
    /// add join the request's Cookie header, unless the request's settings
    /// leave them out.
    pub fn curl_command(
        &self,
        values: &HashMap<String, String>,
        cookies: Option<&CookieJar>,
        preferences: &RequestPreferences,
    ) -> String {
        let mut request = self.clone();
        request.auth = request.auth.sending();

        request.path = keep_unknown(&request.path, values);
        for field in request.headers.iter_mut().chain(request.query.iter_mut()) {
            field.key = keep_unknown(&field.key, values);
            field.value = keep_unknown(&field.value, values);
        }
        for text in request.auth.texts_mut() {
            *text = keep_unknown(text, values);
        }
        match &mut request.body {
            Some(Body::Raw { text, .. }) => *text = keep_unknown(text, values),
            Some(Body::UrlEncoded { fields }) => {
                for (name, value) in fields {
                    *name = keep_unknown(name, values);
                    *value = keep_unknown(value, values);
                }
            }
            Some(Body::Multipart { parts }) => {
                for part in parts {
                    part.name = keep_unknown(&part.name, values);
                    if !part.file {
                        part.value = keep_unknown(&part.value, values);
                    }
                }
            }
            Some(Body::Binary { .. }) | None => {}
        }

        // Filling in a path variable encodes its value, including the braces
        // of an unknown reference. Such references stand in as placeholders
        // that encoding leaves alone, and are written back afterwards.
        let mut references = Vec::new();
        for (_, value) in &mut request.path_variables {
            *value = replace_unknown(value, values, |reference| {
                references.push(format!("{{{{{reference}}}}}"));
                format!("\u{E000}{}\u{E000}", references.len() - 1)
            });
        }

        // A reference without its closing braces cannot be filled in, so the
        // request is written as it is.
        let mut request = request
            .resolve_for_send(&mut VariableResolver::new(values), false)
            .unwrap_or_else(|_| self.clone().prepare_for_send());
        for (index, reference) in references.iter().enumerate() {
            request.path = request
                .path
                .replace(&format!("\u{E000}{index}\u{E000}"), reference);
        }
        let auth = authorization(&mut request);
        // Rows that are switched off are not sent, also when the request is
        // written as it is.
        let mut headers = Field::pairs(&request.headers);
        let url = url(&request.path, &Field::pairs(&request.query));
        let data = request.body.as_ref().map(data).unwrap_or_default();

        let has = |headers: &[(String, String)], name: &str| {
            headers
                .iter()
                .any(|(header, _)| header.eq_ignore_ascii_case(name))
        };

        // cURL names the type of the forms it sends itself. It would send raw
        // text and files as a URL-encoded form, and a form without fields as
        // no body, which sending still names.
        let content_type = match &request.body {
            Some(Body::Raw { text, .. }) if text.is_empty() => None,
            Some(body @ (Body::Raw { .. } | Body::Binary { .. })) => Some(body.content_type()),
            Some(body @ Body::UrlEncoded { fields }) if fields.is_empty() => {
                Some(body.content_type())
            }
            Some(Body::UrlEncoded { .. } | Body::Multipart { .. }) | None => None,
        };
        if let Some(content_type) = content_type
            && !has(&headers, "content-type")
        {
            headers.push(("Content-Type".into(), content_type));
        }

        // cURL would name itself, which some servers turn away.
        if !has(&headers, "user-agent") {
            headers.push(("User-Agent".into(), crate::USER_AGENT.into()));
        }

        // cURL sends no length without a body, which some servers require
        // for these methods.
        if data.is_empty()
            && matches!(request.method, Method::Post | Method::Put | Method::Patch)
            && !has(&headers, "content-length")
            && !has(&headers, "transfer-encoding")
        {
            headers.push(("Content-Length".into(), "0".into()));
        }

        // The jar's cookies for the request's own URL, which cURL sends on
        // to redirects as well. A URL whose host is not known has none.
        if let Some(jar_cookies) = cookies
            .filter(|_| request.settings.send_cookies)
            .and_then(|jar| jar.cookie_header(&url, &headers))
        {
            let own = headers
                .iter_mut()
                .rfind(|(name, _)| name.eq_ignore_ascii_case("cookie"));
            match own {
                Some((_, value)) if !value.trim().is_empty() => {
                    value.push_str("; ");
                    value.push_str(&jar_cookies);
                }
                Some((_, value)) => *value = jar_cookies,
                None => headers.push(("Cookie".into(), jar_cookies)),
            }
        }

        let settings = &request.settings;
        let mut command = String::from("curl");
        if settings
            .follow_redirects
            .unwrap_or(preferences.follow_all_redirects)
        {
            command.push_str(" --location");
        }
        if !settings
            .verify_certificates
            .unwrap_or(preferences.ssl_certificate_verification)
        {
            command.push_str(" --insecure");
        }
        let timeout = settings.timeout_ms.unwrap_or(preferences.timeout_ms);
        if timeout > 0 {
            // In seconds, which may have a fraction.
            command.push_str(&format!(" --max-time {}", timeout as f64 / 1000.));
        }
        match preferences.http_version {
            // cURL also offers HTTP/2 to secure servers and uses HTTP/1.1
            // with others.
            HttpVersion::Auto => {}
            HttpVersion::Http1_1 => command.push_str(" --http1.1"),
            HttpVersion::Http2 => command.push_str(" --http2-prior-knowledge"),
        }
        // cURL would read brackets and braces as patterns of several URLs.
        if url.contains(['[', ']', '{', '}']) {
            command.push_str(" --globoff");
        }
        match request.method {
            Method::Head => command.push_str(" --head"),
            // cURL sends a body with POST unless told otherwise.
            Method::Get => {}
            Method::Post if !data.is_empty() => {}
            method => {
                command.push_str(" --request ");
                command.push_str(method.as_str());
            }
        }
        command.push(' ');
        command.push_str(&quote(&url));

        for (name, value) in &headers {
            // cURL leaves out a header written with an empty value after `:`.
            let header = if value.is_empty() {
                format!("{name};")
            } else {
                format!("{name}: {value}")
            };
            command.push_str(" \\\n--header ");
            command.push_str(&quote(&header));
        }

        for (option, value) in auth {
            command.push_str(" \\\n--");
            command.push_str(option);
            if let Some(value) = value {
                command.push(' ');
                command.push_str(&quote(&value));
            }
        }

        for (option, value) in data {
            command.push_str(" \\\n--");
            command.push_str(option);
            command.push(' ');
            command.push_str(&quote(&value));
        }

        command
    }
}

/// The options that send the request's resolved authorization. cURL
/// computes Basic, Digest and header AWS signatures itself. The credentials
/// of the others, including a presigned AWS query, are added to the request
/// as sending adds them now, over the body cURL sends. One that cannot be
/// made, such as a JWT without its key, is left out.
fn authorization(request: &mut HttpRequest) -> Vec<(&'static str, Option<String>)> {
    let auth = std::mem::take(&mut request.auth);
    let own_authorization = Field::enabled(&request.headers)
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
    let user = |username: &str, password: &str| ("user", Some(format!("{username}:{password}")));

    match &auth {
        _ if own_authorization
            && auth
                .credential_name()
                .is_some_and(|(_, name)| name == "Authorization") =>
        {
            Vec::new()
        }
        Auth::Basic(auth) if auth.username.is_empty() && auth.password.is_empty() => Vec::new(),
        Auth::Basic(auth) => vec![user(&auth.username, &auth.password)],
        Auth::Digest(auth) => vec![("digest", None), user(&auth.username, &auth.password)],
        // A presigned query covers the body unless it is S3's, which cURL
        // reads from files only when it sends them; cURL signs those itself.
        Auth::AwsSignature(aws)
            if aws.add_to == AuthLocation::Header
                || (aws.service.trim() != "s3"
                    && matches!(
                        request.body,
                        Some(Body::Multipart { .. } | Body::Binary { .. })
                    )) =>
        {
            if !aws.session_token.is_empty() {
                request.headers.push(Field::new(
                    "X-Amz-Security-Token",
                    aws.session_token.clone(),
                ));
            }

            vec![
                (
                    "aws-sigv4",
                    Some(format!(
                        "aws:amz:{}:{}",
                        aws.region.trim(),
                        aws.service.trim()
                    )),
                ),
                user(aws.access_key.trim(), aws.secret_key.trim()),
            ]
        }
        auth => {
            // Only a presigned AWS query signs the body.
            let body = match auth {
                Auth::AwsSignature(_) => sent_body(request.body.as_ref()).unwrap_or_default(),
                _ => Vec::new(),
            };
            let form = match &request.body {
                Some(Body::UrlEncoded { fields }) => fields.clone(),
                _ => Vec::new(),
            };
            let _ = crate::auth::authorize(
                auth,
                request.method.as_str(),
                &request.path,
                &mut request.query,
                &mut request.headers,
                &body,
                &form,
            );

            Vec::new()
        }
    }
}

/// The bytes cURL sends for a body that it does not read from files.
pub(crate) fn sent_body(body: Option<&Body>) -> Option<Vec<u8>> {
    match body {
        None => Some(Vec::new()),
        Some(Body::Raw { text, .. }) => Some(text.clone().into_bytes()),
        Some(Body::UrlEncoded { fields }) => Some(
            fields
                .iter()
                .map(|(name, value)| format!("{}={}", form_encode(name), data_urlencode(value)))
                .collect::<Vec<_>>()
                .join("&")
                .into_bytes(),
        ),
        Some(Body::Multipart { .. } | Body::Binary { .. }) => None,
    }
}

/// A form value as `--data-urlencode` encodes it: the unreserved characters
/// stay, a space becomes `+` and the rest is escaped.
fn data_urlencode(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                escaped.push(char::from(byte));
            }
            b' ' => escaped.push('+'),
            byte => {
                let _ = write!(escaped, "%{byte:02X}");
            }
        }
    }
    escaped
}

/// The options that send the body, with their values.
fn data(body: &Body) -> Vec<(&'static str, String)> {
    match body {
        Body::Raw { text, .. } if text.is_empty() => Vec::new(),
        // `--data` reads a file when its value starts with `@`.
        Body::Raw { text, .. } if text.contains('@') => vec![("data-raw", text.clone())],
        Body::Raw { text, .. } => vec![("data", text.clone())],
        // cURL encodes the value, but expects the name to be encoded already.
        // Without a name, it would send the value alone, without `=`.
        Body::UrlEncoded { fields } => fields
            .iter()
            .map(|(name, value)| match name.as_str() {
                "" => ("data-raw", format!("={}", data_urlencode(value))),
                name => ("data-urlencode", format!("{}={value}", form_encode(name))),
            })
            .collect(),
        Body::Multipart { parts } => parts
            .iter()
            .map(|part| {
                if part.file {
                    // A quoted file name may contain `;` and `,`. cURL knows
                    // the types of fewer files than sending does.
                    let path = part.value.replace('\\', "\\\\").replace('"', "\\\"");
                    let file_type = crate::body::file_type(Path::new(&part.value));
                    (
                        "form",
                        format!("{}=@\"{path}\";type={file_type}", part.name),
                    )
                } else {
                    ("form-string", format!("{}={}", part.name, part.value))
                }
            })
            .collect(),
        Body::Binary { file } => vec![("data-binary", format!("@{}", file.display()))],
    }
}

/// The URL without its fragment, with the editor's query parameters encoded
/// after its own, as sending does. Unfilled variables stay readable.
fn url(path: &str, query: &[(String, String)]) -> String {
    let unfilled = path.contains("{{")
        || query
            .iter()
            .any(|(name, value)| name.contains("{{") || value.contains("{{"));

    // Sending parses the URL, which encodes characters such as spaces.
    if !unfilled && let Ok(mut url) = Url::parse(path) {
        url.set_fragment(None);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        return url.into();
    }

    let mut url = path.split('#').next().unwrap_or_default().to_owned();
    if query.is_empty() {
        return url;
    }

    if !url.contains('?') {
        url.push('?');
    } else if !url.ends_with(['?', '&']) {
        url.push('&');
    }

    let pairs = query
        .iter()
        .map(|(name, value)| format!("{}={}", form_encode(name), form_encode(value)))
        .collect::<Vec<_>>();
    url.push_str(&pairs.join("&"));

    url
}

/// Encodes a query parameter as sending does, except its `{{variables}}`.
fn form_encode(text: &str) -> String {
    let mut encoded = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("{{")
        && let Some(length) = rest[start..].find("}}")
    {
        let end = start + length + 2;
        encoded.extend(byte_serialize(&rest.as_bytes()[..start]));
        encoded.push_str(&rest[start..end]);
        rest = &rest[end..];
    }
    encoded.extend(byte_serialize(rest.as_bytes()));

    encoded
}

/// Marks the references `values` cannot fill in as literal, `{{!name}}`, so
/// resolving the request leaves them as written.
pub(crate) fn keep_unknown(text: &str, values: &HashMap<String, String>) -> String {
    replace_unknown(text, values, |reference| format!("{{{{!{reference}}}}}"))
}

/// Replaces each reference `values` cannot fill in with `replace`'s text for
/// it. Generated values such as `{{$guid}}` are new on every send, so they
/// count as unknown too. Literal references, `{{!name}}`, stay as they are.
fn replace_unknown(
    text: &str,
    values: &HashMap<String, String>,
    mut replace: impl FnMut(&str) -> String,
) -> String {
    let mut kept = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("{{")
        && let Some(length) = rest[start + 2..].find("}}")
    {
        let reference = &rest[start + 2..start + 2 + length];
        let end = start + 2 + length + 2;
        let name = reference.trim();
        kept.push_str(&rest[..start]);

        if reference.starts_with('!') || (values.contains_key(name) && !name.starts_with('$')) {
            kept.push_str(&rest[start..end]);
        } else {
            kept.push_str(&replace(reference));
        }

        rest = &rest[end..];
    }
    kept.push_str(rest);

    kept
}

/// Quotes text for a POSIX shell.
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
