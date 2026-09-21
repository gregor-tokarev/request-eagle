use base64::{Engine as _, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;
use url::{Position, Url};

use crate::Method;

/// Defaults shared by request execution and the header editor's preview.
/// Host is conveyed as :authority on HTTP/2. Explicit header names take
/// precedence, case-insensitively. Body length is in bytes, not characters.
pub fn generated_headers(
    method: Method,
    path: &str,
    headers: &[(String, String)],
    body_bytes: usize,
) -> Vec<(String, String)> {
    let has = |name: &str| {
        headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case(name))
    };
    let mut generated = Vec::new();

    if let Ok(url) = Url::parse(path)
        && matches!(url.scheme(), "http" | "https")
    {
        if !has("host") {
            generated.push((
                "Host".into(),
                url[Position::BeforeHost..Position::AfterPort].to_owned(),
            ));
        }

        if !has("authorization")
            && let Ok(username) = percent_decode_str(url.username()).decode_utf8()
        {
            let password = url
                .password()
                .and_then(|value| percent_decode_str(value).decode_utf8().ok());

            if !username.is_empty() || password.is_some() {
                let credentials = format!("{username}:{}", password.as_deref().unwrap_or_default());
                generated.push((
                    "Authorization".into(),
                    format!("Basic {}", STANDARD.encode(credentials)),
                ));
            }
        }
    }

    if !has("accept") {
        generated.push(("Accept".into(), "*/*".into()));
    }

    if !has("content-length")
        && !has("transfer-encoding")
        && (body_bytes > 0 || matches!(method, Method::Post | Method::Put))
    {
        generated.push(("Content-Length".into(), body_bytes.to_string()));
    }

    generated
}
