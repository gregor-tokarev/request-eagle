//! Request bodies shared by the Postman and OpenAPI conversions. Request
//! Eagle stores a body as raw bytes, so forms are encoded here.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use request::Field;

/// Separates multipart fields. Fixed, so that imports are reproducible.
const BOUNDARY: &str = "RequestEagleFormBoundary";

/// Characters left as they are in form fields, as in `encodeURIComponent`'s
/// unreserved set.
const FORM_FIELD: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Fills in a form's variables when sending and then encodes the fields, as
/// Postman does, so that a value such as `a&b` stays in its field.
const ENCODE_FORM_SCRIPT: &str = r#"// Encode the form after filling in its variables, so each value stays in its field.
if (pm.request.body.raw) {
    pm.request.body.raw = pm.request.body.raw
        .split("&")
        .map((field) => field
            .split("=")
            .map((part) => encodeURIComponent(pm.variables.replaceIn(decodeURIComponent(part))))
            .join("="))
        .join("&");
}"#;

/// A URL-encoded form. Its `{{variables}}` are left for
/// [`url_encoded_form_script`] to fill in and encode when sending.
pub(crate) fn url_encoded_form(fields: &[(String, String)], headers: &mut Vec<Field>) -> Vec<u8> {
    set_content_type(headers, "application/x-www-form-urlencoded");

    fields
        .iter()
        .map(|(name, value)| format!("{}={}", encode_form_field(name), encode_form_field(value)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

/// The pre-request script a form with variables needs.
pub(crate) fn url_encoded_form_script(fields: &[(String, String)]) -> Option<String> {
    fields
        .iter()
        .any(|(name, value)| name.contains("{{") || value.contains("{{"))
        .then(|| ENCODE_FORM_SCRIPT.to_owned())
}

/// Percent-encodes text except its `{{variables}}`.
fn encode_form_field(text: &str) -> String {
    let mut encoded = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("{{")
        && let Some(length) = rest[start..].find("}}")
    {
        let end = start + length + 2;
        encoded.extend(utf8_percent_encode(&rest[..start], FORM_FIELD));
        encoded.push_str(&rest[start..end]);
        rest = &rest[end..];
    }
    encoded.extend(utf8_percent_encode(rest, FORM_FIELD));

    encoded
}

/// A multipart form of text fields.
pub(crate) fn multipart_form(fields: &[(String, String)], headers: &mut Vec<Field>) -> Vec<u8> {
    set_content_type(
        headers,
        &format!("multipart/form-data; boundary={BOUNDARY}"),
    );

    let mut body = String::new();
    for (name, value) in fields {
        let name = name.replace('"', "%22");
        body.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{BOUNDARY}--\r\n"));

    body.into_bytes()
}

/// Adds a `Content-Type` header unless the request already has one.
pub(crate) fn set_content_type(headers: &mut Vec<Field>, content_type: &str) {
    if !Field::enabled(headers).any(|(name, _)| name.eq_ignore_ascii_case("content-type")) {
        headers.push(Field::new("Content-Type", content_type));
    }
}
