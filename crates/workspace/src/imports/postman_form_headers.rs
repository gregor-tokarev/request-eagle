use request::{ApiKeyLocation, Authentication, FormBody, HttpRequest};
use serde_json::Value;

pub(super) fn validate(
    form: &FormBody,
    headers: &[(String, String)],
    content_type_override: bool,
) -> Result<(), String> {
    let types = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>();

    if content_type_override {
        // A plain urlencoded header remains identical. Multipart requires an
        // automatically supplied boundary, which this Postman setting disables.
        let unchanged = matches!(form, FormBody::UrlEncoded(_))
            && !types.is_empty()
            && types
                .iter()
                .all(|value| value.trim().eq_ignore_ascii_case(form.content_type()));

        if !unchanged {
            return Err("Postman form imports cannot preserve disabled system Content-Type behavior. Enable the system Content-Type or use a raw body.".into());
        }
    }

    for value in types {
        if value.contains("{{") {
            return Err("Postman form imports cannot preserve a templated Content-Type. Resolve the header value before importing.".into());
        }

        let preserved_override = match form {
            FormBody::UrlEncoded(_) => value
                .strip_prefix("application/x-www-form-urlencoded")
                .is_some_and(|suffix| {
                    // Postman uses a case-sensitive word boundary after the
                    // media type, preserving both parameters and '-extra'.
                    suffix
                        .chars()
                        .next()
                        .is_some_and(|first| !first.is_ascii_alphanumeric() && first != '_')
                        && suffix.split(';').any(|part| !part.trim().is_empty())
                }),
            FormBody::Multipart(_) => {
                value
                    .strip_prefix("multipart/form-data;")
                    .is_some_and(|parameters| {
                        parameters.match_indices("boundary=").any(|(index, _)| {
                            let boundary = &parameters[index + "boundary=".len()..];
                            !boundary.is_empty() && !boundary.starts_with(';')
                        })
                    })
            }
        };

        if preserved_override {
            return Err("Postman form imports cannot preserve custom Content-Type parameters or boundaries. Use the bare form Content-Type or a raw body.".into());
        }
    }

    Ok(())
}

pub(super) fn validate_request(
    request: &HttpRequest,
    content_type_override: bool,
) -> Result<(), String> {
    if let Authentication::ApiKey {
        name,
        value,
        location: ApiKeyLocation::Header,
    } = &request.authentication
    {
        if name.contains("{{") && (request.form.is_some() || content_type_override) {
            return Err("Postman form or suppressed Content-Type imports require a literal API-key header name before its Content-Type behavior can be preserved.".into());
        }

        if name.eq_ignore_ascii_case("content-type") {
            // Postman marks helper headers as system-owned, so this setting
            // removes the helper's Content-Type even when it is explicit.
            if content_type_override {
                return Err("Postman imports cannot preserve an API-key Content-Type header while the system Content-Type is disabled.".into());
            }

            if let Some(form) = &request.form {
                let mut headers = request.headers.clone();
                headers.push((name.clone(), value.clone()));
                return validate(form, &headers, false);
            }
        }
    }

    if let Some(form) = &request.form {
        validate(form, &request.headers, content_type_override)?;
    }

    Ok(())
}

pub(super) fn validate_raw(
    source_headers: Option<&Value>,
    headers: &[(String, String)],
) -> Result<(), String> {
    // Runtime selects the last enabled header. A header marked as system-owned
    // causes it to suppress every Content-Type header, including earlier ones.
    let explicit = super::postman_profiles::last_header(source_headers, headers, "content-type")
        == Some(false);

    if !explicit {
        return Err("Postman raw-body imports cannot preserve a disabled system Content-Type without an enabled explicit Content-Type header. Enable the system header or add an explicit Content-Type before importing.".into());
    }

    Ok(())
}
