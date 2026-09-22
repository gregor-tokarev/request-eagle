use request::{HttpRequest, Method};

/// An imported request, before the user chooses a collection to save it in.
#[derive(Debug)]
pub(crate) struct ImportedRequest {
    pub name: String,
    pub folders: Vec<String>,
    pub request: HttpRequest,
}

pub(crate) fn parse_import(input: &str) -> Result<Vec<ImportedRequest>, String> {
    let input = input.trim().trim_start_matches('\u{feff}');

    if input.is_empty() {
        return Err("Paste a cURL command or a Postman collection JSON file.".into());
    }

    if input.len() > 16 * 1024 * 1024 {
        return Err("Imports must be 16 MiB or smaller.".into());
    }

    if input.starts_with('{') {
        super::postman::parse(input)
    } else {
        super::curl::parse(input).map(|request| vec![request])
    }
}

pub(super) fn method(value: &str) -> Result<Method, String> {
    serde_json::from_value(serde_json::Value::String(value.to_ascii_uppercase()))
        .map_err(|_| format!("HTTP method {value:?} is not supported."))
}

pub(super) fn add_content_type(request: &mut HttpRequest, value: &str) {
    if !request
        .headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-type"))
    {
        request.headers.push(("Content-Type".into(), value.into()));
    }
}

pub(super) fn header(value: &str) -> Result<(String, String), String> {
    if let Some((key, value)) = value.split_once(':') {
        if key.trim().is_empty() {
            return Err("An imported header has an empty name.".into());
        }

        return Ok((key.trim().into(), value.trim_start().into()));
    }

    Err(format!("Header {value:?} must contain a name and a colon."))
}
