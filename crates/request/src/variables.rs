use environment::{VariableError, VariableResolver, VariableValues};

use crate::HttpRequest;

impl HttpRequest {
    /// Resolve a send snapshot, preserving the saved request and editable draft.
    pub fn resolve_variables(&self, values: &VariableValues) -> Result<Self, VariableError> {
        let mut resolver = VariableResolver::new(values);
        let mut request = self.clone();
        request.path = resolve_url(&request.path, &mut resolver)?;

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
        resolved.push_str(&remaining[..start]);
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
    resolved.push_str(remaining);
    Ok(resolved)
}
