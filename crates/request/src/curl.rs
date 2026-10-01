//! Requests written as cURL commands, in the form of Postman's cURL code
//! snippets: long option names, single quotes and one option per line.

use std::collections::HashMap;

use url::form_urlencoded::byte_serialize;

use crate::{HttpRequest, Method};

impl HttpRequest {
    /// The cURL command that sends this request as Request Eagle does,
    /// following redirects. `{{variables}}` that `values` defines are filled
    /// in; others, and generated ones such as `{{$guid}}`, stay as written.
    pub fn curl_command(&self, values: &HashMap<String, String>) -> String {
        let mut request = self.clone();
        request.path = fill_variables(&request.path, values);
        for (key, value) in request.headers.iter_mut().chain(request.query.iter_mut()) {
            *key = fill_variables(key, values);
            *value = fill_variables(value, values);
        }
        request.body = request
            .body
            .map(|body| fill_variables(&String::from_utf8_lossy(&body), values).into_bytes());

        let request = request.prepare_for_send();
        let body = request.body.as_deref().filter(|body| !body.is_empty());
        let mut command = String::from("curl --location");
        match request.method {
            Method::Head => command.push_str(" --head"),
            // cURL sends a body with POST unless told otherwise.
            Method::Get => {}
            Method::Post if body.is_some() => {}
            method => {
                command.push_str(" --request ");
                command.push_str(method.as_str());
            }
        }
        command.push(' ');
        command.push_str(&quote(&url(&request.path, &request.query)));

        for (name, value) in &request.headers {
            // cURL leaves out a header written with an empty value after `:`.
            let header = if value.is_empty() {
                format!("{name};")
            } else {
                format!("{name}: {value}")
            };
            command.push_str(" \\\n--header ");
            command.push_str(&quote(&header));
        }

        if let Some(body) = body {
            let body = String::from_utf8_lossy(body);
            // `--data` reads a file when its value starts with `@`.
            command.push_str(if body.contains('@') {
                " \\\n--data-raw "
            } else {
                " \\\n--data "
            });
            command.push_str(&quote(&body));
        }

        command
    }
}

/// The URL without its fragment, with the editor's query parameters encoded
/// after its own, as sending does. Unfilled variables stay readable.
fn url(path: &str, query: &[(String, String)]) -> String {
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

fn fill_variables(text: &str, values: &HashMap<String, String>) -> String {
    let mut filled = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("{{")
        && let Some(length) = rest[start + 2..].find("}}")
    {
        let reference = &rest[start + 2..start + 2 + length];
        filled.push_str(&rest[..start]);

        // `{{!name}}` writes `{{name}}` itself.
        if let Some(literal) = reference.strip_prefix('!') {
            filled.push_str(&format!("{{{{{literal}}}}}"));
        } else {
            let name = reference.trim();
            match values.get(name).filter(|_| !name.starts_with('$')) {
                Some(value) => filled.push_str(value),
                None => filled.push_str(&rest[start..start + 2 + length + 2]),
            }
        }

        rest = &rest[start + 2 + length + 2..];
    }
    filled.push_str(rest);

    filled
}

/// Quotes text for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}
