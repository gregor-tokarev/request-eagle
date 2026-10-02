//! Request bodies shared by the Postman and OpenAPI conversions.

/// Adds a `Content-Type` header unless the request already has one.
pub(crate) fn set_content_type(headers: &mut Vec<(String, String)>, content_type: &str) {
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), content_type.into()));
    }
}
