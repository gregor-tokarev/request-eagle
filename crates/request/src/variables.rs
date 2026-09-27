use environment::{VariableError, VariableResolver, VariableValues};

use crate::HttpRequest;

/// A collection-variable snapshot and any failure to read its source.
pub struct RequestVariables {
    pub(crate) values: VariableValues,
    environment_error: Option<String>,
}

impl RequestVariables {
    pub fn new(mut values: VariableValues, environment_error: Option<String>) -> Self {
        if environment_error.is_some() {
            values.environment.clear();
        } else {
            values.environment.retain(|name, _| !name.starts_with('$'));
        }
        Self {
            values,
            environment_error,
        }
    }

    pub fn resolve(&self, request: &HttpRequest) -> Result<HttpRequest, String> {
        self.resolve_owned(request.clone(), false)
    }

    pub(crate) fn resolve_owned(
        &self,
        request: HttpRequest,
        body_changed: bool,
    ) -> Result<HttpRequest, String> {
        let mut resolver = VariableResolver::new(&self.values);
        if !request.scripts.is_empty() {
            resolver.limit_output(32 * 1024 * 1024);
        }
        // Only scripts can introduce these reserved names; collection values
        // were filtered when this send snapshot was created.
        if !request.scripts.pre_request.trim().is_empty() {
            for (name, value) in &self.values.environment {
                if name.starts_with('$') {
                    resolver.override_generated(name.clone(), value.clone());
                }
            }
        }
        request
            .resolve_with(&mut resolver, body_changed)
            .map_err(|error| {
                if let VariableError::Unknown(name) = &error
                    && !name.starts_with('$')
                    && let Some(message) = &self.environment_error
                {
                    return message.clone();
                }
                error.to_string()
            })
    }
}

impl HttpRequest {
    /// Resolve a send snapshot, preserving the saved request and editable draft.
    pub fn resolve_variables(&self, values: &VariableValues) -> Result<Self, VariableError> {
        self.clone()
            .resolve_with(&mut VariableResolver::new(values), false)
    }

    fn resolve_with(
        self,
        resolver: &mut VariableResolver<'_>,
        body_changed: bool,
    ) -> Result<Self, VariableError> {
        let mut request = self;
        request.path = resolve_url(&request.path, resolver)?;

        for (key, value) in request
            .headers
            .iter_mut()
            .chain(request.query.iter_mut().flatten())
        {
            *key = resolver.resolve(key)?;
            *value = resolver.resolve(value)?;
        }

        if let Some(body) = &mut request.body
            && let Ok(text) = std::str::from_utf8(body)
            && (body_changed || text.contains("{{"))
        {
            *body = resolver.resolve(text)?.into_bytes();
        }

        Ok(request)
    }
}

fn resolve_url(text: &str, resolver: &mut VariableResolver<'_>) -> Result<String, VariableError> {
    let mut remaining = text.split('#').next().unwrap_or_default();
    let mut resolved = String::new();

    while let Some(start) = remaining.find("{{") {
        resolved.push_str(&resolver.resolve(&remaining[..start])?);
        let end = remaining[start + 2..]
            .find("}}")
            .map(|end| start + 2 + end + 2)
            .ok_or(VariableError::Unclosed)?;
        let value = resolver.resolve(&remaining[start..end])?;
        // A whole-URL variable can introduce a fragment too. No later reference
        // in this URL is transmitted, so do not resolve or validate it.
        if let Some((before_fragment, _)) = value.split_once('#') {
            resolved.push_str(before_fragment);
            return Ok(resolved);
        }
        resolved.push_str(&value);
        remaining = &remaining[end..];
    }
    resolved.push_str(&resolver.resolve(remaining)?);
    Ok(resolved)
}
