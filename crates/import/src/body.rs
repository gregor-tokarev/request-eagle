//! Request bodies shared by the Postman and OpenAPI conversions. Request
//! Eagle stores a body as raw bytes, so forms are encoded here.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Separates multipart fields. Fixed, so that imports are reproducible.
const BOUNDARY: &str = "RequestEagleFormBoundary";

/// Leaves `{{variables}}` readable and resolvable in encoded forms.
const FORM_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'{')
    .remove(b'}');

pub(crate) fn url_encoded_form(
    fields: &[(String, String)],
    headers: &mut Vec<(String, String)>,
) -> Vec<u8> {
    set_content_type(headers, "application/x-www-form-urlencoded");

    fields
        .iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                utf8_percent_encode(name, FORM_VALUE),
                utf8_percent_encode(value, FORM_VALUE)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

/// A multipart form of text fields.
pub(crate) fn multipart_form(
    fields: &[(String, String)],
    headers: &mut Vec<(String, String)>,
) -> Vec<u8> {
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
pub(crate) fn set_content_type(headers: &mut Vec<(String, String)>, content_type: &str) {
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), content_type.into()));
    }
}
