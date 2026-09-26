use environment::{VariableError, VariableResolver, VariableValues};

use crate::HttpRequest;

impl HttpRequest {
    /// Resolve a send snapshot, preserving the saved request and editable draft.
    pub fn resolve_variables(&self, values: &VariableValues) -> Result<Self, VariableError> {
        let mut resolver = VariableResolver::new(values);
        let mut request = self.clone();
        request.path = resolver.resolve(&request.path)?;

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
