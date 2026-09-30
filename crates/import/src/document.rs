use collection::ImportedCollection;
use serde_json::Value;
use thiserror::Error;

use crate::{openapi, postman};

/// A converted collection and what could not be converted.
pub struct Import {
    pub collection: ImportedCollection,
    /// Requests left out because Request Eagle cannot send their HTTP method.
    pub skipped: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("The file is not valid JSON or YAML: {0}")]
    Syntax(String),
    #[error("The file is not a Postman collection or an OpenAPI specification.")]
    UnknownFormat,
    #[error("Postman Collection v1 is not supported. Export the collection as v2.1 and try again.")]
    PostmanV1,
    #[error("OpenAPI {0} is not supported. Use OpenAPI 3 or Swagger 2.0.")]
    UnsupportedOpenApi(String),
}

/// Converts a Postman collection or an OpenAPI specification, recognized by
/// its content, into a collection.
pub fn parse(source: &str) -> Result<Import, ImportError> {
    let source = source.trim_start_matches('\u{feff}');

    // YAML also reads JSON, but JSON's errors are clearer for JSON files.
    let document: Value = if source.trim_start().starts_with(['{', '[']) {
        serde_json::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))?
    } else {
        serde_saphyr::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))?
    };

    if document.get("openapi").is_some() || document.get("swagger").is_some() {
        openapi::convert(&document)
    } else if document.get("info").is_some() && document.get("item").is_some() {
        postman::convert(&document)
    } else if document.get("requests").is_some() && document.get("order").is_some() {
        Err(ImportError::PostmanV1)
    } else {
        Err(ImportError::UnknownFormat)
    }
}

/// A single-line name, or `fallback` when there is none.
pub(crate) fn clean_name(name: Option<&str>, fallback: &str) -> String {
    let name = name
        .unwrap_or_default()
        .split(char::is_control)
        .collect::<Vec<_>>()
        .join(" ");
    let name = name.trim();

    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.to_owned()
    }
}

/// A scalar as text; numbers and booleans are written as they appear in JSON.
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(value) => value.to_string(),
    }
}
