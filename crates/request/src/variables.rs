use std::collections::BTreeMap;
use std::collections::HashMap;

use environment::{EnvironmentSession, VariableError, VariableResolver};

use crate::{HttpRequest, RequestScripts, WebSocketRequest};

/// A collection-variable snapshot and any failure to read its source.
pub struct RequestVariables {
    pub(crate) values: HashMap<String, String>,
    pub(crate) session: Option<EnvironmentSession>,
    pub(crate) collection_scripts: Result<RequestScripts, String>,
    pub(crate) environment_error: Option<String>,
}

impl RequestVariables {
    pub fn new(mut values: HashMap<String, String>, environment_error: Option<String>) -> Self {
        if environment_error.is_some() {
            values.clear();
        } else {
            values.retain(|name, _| !name.starts_with('$'));
        }
        Self {
            values,
            session: None,
            collection_scripts: Ok(RequestScripts::default()),
            environment_error,
        }
    }

    /// Run the collection's scripts before the request's own script in each
    /// phase. A failure to read them stops the send before any script runs.
    pub fn with_collection_scripts(mut self, scripts: Result<RequestScripts, String>) -> Self {
        self.collection_scripts = scripts;
        self
    }

    /// Read the current session overlay while retaining file-read errors for
    /// references that cannot be satisfied by the session itself.
    pub fn with_environment_session(
        values: HashMap<String, String>,
        environment_error: Option<String>,
        session: EnvironmentSession,
    ) -> Self {
        let mut variables = Self::new(values, environment_error);
        variables.values = session.values(variables.values);
        variables.values.retain(|name, _| !name.starts_with('$'));
        variables.session = Some(session);

        variables
    }

    pub fn resolve(&self, request: &HttpRequest) -> Result<HttpRequest, String> {
        let scripted = !request.scripts.pre_request.trim().is_empty();
        resolve_request(
            &self.values,
            self.environment_error.as_deref(),
            request.clone(),
            scripted,
            false,
            &mut BTreeMap::new(),
        )
    }

    /// Resolve the URL, parameters and headers a WebSocket connects with.
    pub(crate) fn resolve_websocket(
        &self,
        request: &WebSocketRequest,
    ) -> Result<WebSocketRequest, String> {
        let mut resolver = VariableResolver::new(&self.values);
        let mut request = request.clone();
        let mut resolve = || {
            request.url = resolve_url(&request.url, &mut resolver)?;

            for (key, value) in request.headers.iter_mut().chain(request.query.iter_mut()) {
                *key = resolver.resolve(key)?;
                *value = resolver.resolve(value)?;
            }

            Ok(())
        };

        resolve()
            .map(|()| request)
            .map_err(|error| describe_error(error, self.environment_error.as_deref()))
    }

    /// Resolve one outgoing WebSocket message.
    pub(crate) fn resolve_text(&self, text: &str) -> Result<String, String> {
        VariableResolver::new(&self.values)
            .resolve(text)
            .map_err(|error| describe_error(error, self.environment_error.as_deref()))
    }
}

/// An unknown variable is most likely missing because its environment could
/// not be read, so report that instead.
fn describe_error(error: VariableError, environment_error: Option<&str>) -> String {
    if let VariableError::Unknown(name) = &error
        && !name.starts_with('$')
        && let Some(message) = environment_error
    {
        return message.to_owned();
    }

    error.to_string()
}

/// `scripted` reports whether a collection or request pre-request script ran.
pub(crate) fn resolve_request(
    values: &HashMap<String, String>,
    environment_error: Option<&str>,
    request: HttpRequest,
    scripted: bool,
    body_changed: bool,
    generated: &mut BTreeMap<String, String>,
) -> Result<HttpRequest, String> {
    let mut resolver = VariableResolver::new(values);
    for (name, value) in generated.iter() {
        resolver.override_generated(name.clone(), value.clone());
    }
    if scripted || !request.scripts.is_empty() {
        resolver.limit_output(32 * 1024 * 1024);
    }
    // Only scripts can introduce these reserved names; collection values
    // were filtered when this send snapshot was created.
    if scripted {
        for (name, value) in values {
            if name.starts_with('$') {
                resolver.override_generated(name.clone(), value.clone());
            }
        }
    }
    let resolved = request.resolve_with(&mut resolver, body_changed);
    // Keep generated values for the post-response phase, separate from
    // local overrides so unsetting an override restores the cached value.
    for (name, value) in resolver.generated_values() {
        if !values.contains_key(name) {
            generated.insert(name.clone(), value.clone());
        }
    }
    resolved.map_err(|error| describe_error(error, environment_error))
}

impl HttpRequest {
    /// Resolve a send snapshot, preserving the saved request and editable draft.
    pub fn resolve_variables(
        &self,
        values: &HashMap<String, String>,
    ) -> Result<Self, VariableError> {
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

        for (key, value) in request.headers.iter_mut().chain(request.query.iter_mut()) {
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
