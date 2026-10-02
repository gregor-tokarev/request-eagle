//! Request bodies shared by the Postman and OpenAPI conversions.

use request::Field;

/// Adds a `Content-Type` header unless the request already has one.
pub(crate) fn set_content_type(headers: &mut Vec<Field>, content_type: &str) {
    if !Field::enabled(headers).any(|(name, _)| name.eq_ignore_ascii_case("content-type")) {
        headers.push(Field::new("Content-Type", content_type));
    }
}
