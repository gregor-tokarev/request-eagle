use std::collections::HashMap;

use thiserror::Error;

use crate::{Authentication, HttpRequest};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VariableError {
    #[error("environment variable '{{{{{name}}}}}' in {field} is not defined")]
    Undefined { name: String, field: &'static str },

    #[error("invalid environment variable placeholder in {field}; use {{{{name}}}}")]
    InvalidPlaceholder { field: &'static str },
}

/// Resolves `{{name}}` placeholders in a copy, preserving the saved template.
/// Values are substituted once. Binary bodies are sent without modification.
pub fn resolve_variables(
    request: &HttpRequest,
    variables: &HashMap<String, String>,
) -> Result<HttpRequest, VariableError> {
    let mut resolved = request.clone();
    resolved.path = substitute(&request.path, variables, "URL")?;

    for (name, value) in &mut resolved.headers {
        *name = substitute(name, variables, "header name")?;
        *value = substitute(value, variables, "header value")?;
    }

    if let Some(query) = &mut resolved.query {
        for (name, value) in query {
            *name = substitute(name, variables, "query parameter name")?;
            *value = substitute(value, variables, "query parameter value")?;
        }
    }

    if let Some(body) = &request.body
        && let Ok(body) = std::str::from_utf8(body)
    {
        resolved.body = Some(substitute(body, variables, "body")?.into_bytes());
    }

    match &mut resolved.authentication {
        Authentication::None => {}
        Authentication::Basic { username, password } => {
            *username = substitute(username, variables, "authentication username")?;
            *password = substitute(password, variables, "authentication password")?;
        }
        Authentication::Bearer { token } => {
            *token = substitute(token, variables, "authentication token")?;
        }
        Authentication::ApiKey { name, value, .. } => {
            *name = substitute(name, variables, "API key name")?;
            *value = substitute(value, variables, "API key value")?;
        }
    }

    Ok(resolved)
}

fn substitute(
    input: &str,
    variables: &HashMap<String, String>,
    field: &'static str,
) -> Result<String, VariableError> {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some((literal, placeholder)) = remaining.split_once("{{") {
        output.push_str(literal);

        let (name, suffix) = placeholder
            .split_once("}}")
            .ok_or(VariableError::InvalidPlaceholder { field })?;
        let name = name.trim();

        if name.is_empty() || name.contains(['{', '}']) {
            return Err(VariableError::InvalidPlaceholder { field });
        }

        let value = variables
            .get(name)
            .ok_or_else(|| VariableError::Undefined {
                name: name.to_owned(),
                field,
            })?;
        output.push_str(value);
        remaining = suffix;
    }

    output.push_str(remaining);

    Ok(output)
}
