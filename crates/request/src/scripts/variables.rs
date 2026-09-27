use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::HttpRequest;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Variables {
    pub values: BTreeMap<String, String>,
    pub generated: BTreeMap<String, String>,
}

pub(super) use environment::generate_variable as dynamic_variable;

pub(super) fn needs_variable_expansion(request: &HttpRequest) -> bool {
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
        .any(|text| text.contains("{{$") || text.contains("{{!"))
}

pub(super) fn expand_request(
    request: &mut HttpRequest,
    variables: &mut Variables,
    body_changed: bool,
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
        && (body_changed || text.contains("{{"))
    {
        request.body = Some(replace_variables(text, variables, &mut budget)?.into_bytes());
    }

    Ok(())
}

fn replace_variables(
    text: &str,
    variables: &mut Variables,
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
        if let Some(literal) = name.strip_prefix('!') {
            append("{{")?;
            append(literal)?;
            append("}}")?;
        } else {
            match variables
                .values
                .get(name)
                .or_else(|| variables.generated.get(name))
            {
                Some(value) => append(value)?,
                None => match dynamic_variable(name) {
                    Some(value) => {
                        append(&value)?;
                        variables.generated.insert(name.to_owned(), value);
                    }
                    None => append(&rest[..end + 2])?,
                },
            }
        }
        rest = &rest[end + 2..];
    }

    append(rest)?;
    Ok(result)
}
