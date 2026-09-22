use request::FormBody;
use serde_json::Value;

pub(super) fn content_type_override(item: &Value, inherited: bool) -> bool {
    let Some(headers) = item
        .get("protocolProfileBehavior")
        .and_then(|behavior| behavior.get("disabledSystemHeaders"))
    else {
        return inherited;
    };

    // Postman merges profile properties shallowly, so this entire map replaces
    // an inherited map. Its content-type flag uses JavaScript truthiness.
    match headers.get("content-type") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

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
