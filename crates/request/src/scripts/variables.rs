use std::collections::BTreeMap;

use crate::HttpRequest;

pub(super) type Variables = BTreeMap<String, String>;

pub(super) fn dynamic_variable(name: &str) -> Option<String> {
    match name {
        "$guid" | "$randomUUID" => Some(uuid::Uuid::new_v4().to_string()),
        "$timestamp" => Some(chrono::Utc::now().timestamp().to_string()),
        "$isoTimestamp" => {
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        }
        "$randomInt" => Some(rand::random_range(0..=1000).to_string()),
        _ => None,
    }
}

pub(super) fn has_dynamic_placeholders(request: &HttpRequest) -> bool {
    std::iter::once(request.path.as_str())
        .chain(
            request
                .headers
                .iter()
                .chain(request.query.iter().flatten())
                .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
        )
        .chain(
            request
                .body
                .as_deref()
                .and_then(|body| std::str::from_utf8(body).ok()),
        )
        .any(|text| text.contains("{{$"))
}

pub(super) fn expand_request(
    request: &mut HttpRequest,
    variables: &Variables,
) -> Result<(), String> {
    // Share the limit across all fields, including generated dynamic values.
    let mut budget = 32 * 1024 * 1024;
    request.path = replace_variables(&request.path, variables, &mut budget)?;

    for (key, value) in request
        .headers
        .iter_mut()
        .chain(request.query.iter_mut().flatten())
    {
        *key = replace_variables(key, variables, &mut budget)?;
        *value = replace_variables(value, variables, &mut budget)?;
    }

    // Preserve binary bodies unless a script explicitly replaced them.
    if let Some(body) = &request.body
        && let Ok(text) = std::str::from_utf8(body)
    {
        request.body = Some(replace_variables(text, variables, &mut budget)?.into_bytes());
    }

    Ok(())
}

fn replace_variables(
    text: &str,
    variables: &Variables,
    budget: &mut usize,
) -> Result<String, String> {
    let mut result = String::new();
    let mut append = |text: &str| -> Result<(), String> {
        *budget = budget
            .checked_sub(text.len())
            .ok_or("Expanded request exceeds the 32 MiB script output limit")?;
        result.push_str(text);
        Ok(())
    };
    let mut rest = text;

    while let Some(start) = rest.find("{{") {
        append(&rest[..start])?;
        rest = &rest[start..];
        let Some(end) = rest.find("}}") else { break };
        let name = &rest[2..end];
        match variables.get(name) {
            Some(value) => append(value)?,
            None => append(
                dynamic_variable(name)
                    .as_deref()
                    .unwrap_or(&rest[..end + 2]),
            )?,
        }
        rest = &rest[end + 2..];
    }

    append(rest)?;
    Ok(result)
}
