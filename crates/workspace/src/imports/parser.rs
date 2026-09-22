use request::{HttpRequest, Method};
use std::path::PathBuf;

/// An imported request before storage assigns its native collection and file.
#[derive(Debug)]
pub(crate) struct ImportedRequest {
    pub name: String,
    pub folders: Vec<String>,
    pub request: HttpRequest,
}

pub(crate) fn parse_import(input: &str) -> Result<Vec<ImportedRequest>, String> {
    let input = input
        .trim_matches([' ', '\t', '\n'])
        .trim_start_matches('\u{feff}');
    let json_input = input.trim_matches([' ', '\t', '\n', '\r']);

    if input.is_empty() {
        return Err("Paste a cURL command or a Postman collection JSON file.".into());
    }

    if input.len() > 16 * 1024 * 1024 {
        return Err("Imports must be 16 MiB or smaller.".into());
    }

    let requests = if json_input.starts_with('{') {
        super::postman::parse(json_input)?
    } else {
        vec![super::curl::parse(input)?]
    };

    if let Some(imported) = requests.iter().find(|imported| {
        matches!(imported.request.method, Method::Get | Method::Head)
            && (imported.request.body.is_some() || imported.request.form.is_some())
    }) {
        return Err(format!(
            "{}: {} request bodies are not supported by the editor. Remove the body or choose a body-capable method before importing.",
            imported.name,
            imported.request.method.as_str()
        ));
    }

    Ok(requests)
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

pub(super) fn upload_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);

    if !path.is_absolute() && !value.starts_with("{{") {
        return Err("Imported uploads need an absolute file path or an environment variable containing one.".into());
    }

    Ok(path)
}
