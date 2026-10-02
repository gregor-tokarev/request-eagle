//! Requests written as cURL commands, in the form of Postman's cURL code
//! snippets: long option names, single quotes and one option per line.

use std::collections::HashMap;

use url::{Url, form_urlencoded::byte_serialize};

use crate::{Body, CookieJar, Field, HttpRequest, Method};

impl HttpRequest {
    /// The cURL command that sends this request as Request Eagle does,
    /// following redirects and with the timeout and certificate checks of the
    /// request's settings. `{{variables}}` that `values` defines are filled
    /// in as sending fills them, including `:name` path variables; others,
    /// and generated ones such as `{{$guid}}`, stay as written. The cookies
    /// that `cookies` would add join the request's Cookie header.
    pub fn curl_command(
        &self,
        values: &HashMap<String, String>,
        cookies: Option<&CookieJar>,
    ) -> String {
        let mut request = self.clone();
        // Sending leaves these bodies out before it resolves anything.
        if matches!(request.method, Method::Get | Method::Head) {
            request.body = None;
        }

        request.path = keep_unknown(&request.path, values);
        for field in request.headers.iter_mut().chain(request.query.iter_mut()) {
            field.key = keep_unknown(&field.key, values);
            field.value = keep_unknown(&field.value, values);
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
            .resolve_variables(values)
            .unwrap_or_else(|_| self.clone())
            .prepare_for_send();
        for (index, reference) in references.iter().enumerate() {
            request.path = request
                .path
                .replace(&format!("\u{E000}{index}\u{E000}"), reference);
        }
        // Rows that are switched off are not sent, also when the request is
        // written as it is.
        let mut headers = Field::pairs(&request.headers);
        let url = url(&request.path, &Field::pairs(&request.query));
        let data = request.body.as_ref().map(data).unwrap_or_default();

        // cURL names the type of forms itself; it would send raw text and
        // files as a URL-encoded form.
        if let Some(body @ (Body::Raw { .. } | Body::Binary { .. })) = &request.body
            && !data.is_empty()
            && !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            headers.push(("Content-Type".into(), body.content_type()));
        }

        // The jar's cookies for the request's own URL, which cURL sends on
        // to redirects as well. A URL whose host is not known has none.
        if let Some(jar_cookies) = cookies.and_then(|jar| jar.cookie_header(&url, &headers)) {
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
        if settings.follow_redirects != Some(false) {
            command.push_str(" --location");
        }
        if settings.verify_certificates == Some(false) {
            command.push_str(" --insecure");
        }
        if let Some(timeout) = settings.timeout_ms.filter(|timeout| *timeout > 0) {
            // In seconds, which may have a fraction.
            command.push_str(&format!(" --max-time {}", timeout as f64 / 1000.));
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

        for (option, value) in data {
            command.push_str(" \\\n--");
            command.push_str(option);
            command.push(' ');
            command.push_str(&quote(&value));
        }

        command
    }
}

/// The options that send the body, with their values.
fn data(body: &Body) -> Vec<(&'static str, String)> {
    match body {
        Body::Raw { text, .. } if text.is_empty() => Vec::new(),
        // `--data` reads a file when its value starts with `@`.
        Body::Raw { text, .. } if text.contains('@') => vec![("data-raw", text.clone())],
        Body::Raw { text, .. } => vec![("data", text.clone())],
        // cURL encodes the value, but expects the name to be encoded already.
        Body::UrlEncoded { fields } => fields
            .iter()
            .map(|(name, value)| ("data-urlencode", format!("{}={value}", form_encode(name))))
            .collect(),
        Body::Multipart { parts } => parts
            .iter()
            .map(|part| {
                if part.file {
                    // A quoted file name may contain `;` and `,`.
                    let path = part.value.replace('\\', "\\\\").replace('"', "\\\"");
                    ("form", format!("{}=@\"{path}\"", part.name))
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
