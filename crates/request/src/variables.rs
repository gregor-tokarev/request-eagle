use std::collections::BTreeMap;
use std::collections::HashMap;

use environment::{EnvironmentSession, VariableError, VariableResolver, VariableScopes};
use url::form_urlencoded;

use crate::{Auth, Body, Field, GrpcRequest, HttpRequest, RequestScripts, WebSocketRequest};

/// A collection-variable snapshot and any failure to read its source.
pub struct RequestVariables {
    /// The values `{{name}}` resolves to.
    pub(crate) values: HashMap<String, String>,
    /// The scopes `values` come from, which scripts read and change.
    pub(crate) scopes: VariableScopes,
    pub(crate) session: Option<EnvironmentSession>,
    pub(crate) collection_scripts: Result<RequestScripts, String>,
    /// What requests that inherit their authorization send.
    pub(crate) collection_auth: Auth,
    pub(crate) environment_error: Option<String>,
    /// Values `{{$name}}` resolves to instead of generating new ones, set by
    /// a gRPC call's Before invoke script for the whole call.
    pub(crate) generated: BTreeMap<String, String>,
}

impl RequestVariables {
    /// Environment values without a session.
    pub fn new(mut values: HashMap<String, String>, environment_error: Option<String>) -> Self {
        if environment_error.is_some() {
            values.clear();
        }
        let scopes = VariableScopes {
            environment: values
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect(),
            ..Default::default()
        };

        Self::from_scopes(scopes, environment_error, None)
    }

    /// Run the collection's scripts before the request's own script in each
    /// phase. A failure to read them stops the send before any script runs.
    pub fn with_collection_scripts(mut self, scripts: Result<RequestScripts, String>) -> Self {
        self.collection_scripts = scripts;
        self
    }

    /// Requests that inherit their authorization send the collection's.
    pub fn with_collection_auth(mut self, auth: Auth) -> Self {
        self.collection_auth = auth;
        self
    }

    /// The authorization a request sends: its own, or its collection's
    /// when it inherits it, with only the fields that sending uses.
    pub fn effective_auth(&self, auth: &Auth) -> Auth {
        match auth {
            Auth::Inherit => self.collection_auth.sending(),
            auth => auth.sending(),
        }
    }

    /// Read the collection's variables and the active environment with the
    /// session's changes over them, and the session's globals. Session
    /// values still resolve when a file could not be read.
    pub fn with_environment_session(
        collection: HashMap<String, String>,
        environment: HashMap<String, String>,
        environment_error: Option<String>,
        session: EnvironmentSession,
    ) -> Self {
        let scopes = if environment_error.is_some() {
            session.scopes(HashMap::new(), HashMap::new())
        } else {
            session.scopes(collection, environment)
        };

        Self::from_scopes(scopes, environment_error, Some(session))
    }

    fn from_scopes(
        mut scopes: VariableScopes,
        environment_error: Option<String>,
        session: Option<EnvironmentSession>,
    ) -> Self {
        // `{{$name}}` generates a value unless a script sets one for the send.
        scopes.retain(|name| !name.starts_with('$'));

        Self {
            values: scopes.values(),
            scopes,
            session,
            collection_scripts: Ok(RequestScripts::default()),
            collection_auth: Auth::Inherit,
            environment_error,
            generated: BTreeMap::new(),
        }
    }

    /// Resolve where a gRPC request connects: its URL and metadata. The
    /// message is left as written.
    pub fn resolve_grpc_target(&self, request: &GrpcRequest) -> Result<GrpcRequest, String> {
        self.resolve_grpc(request, false)
            .map(|(request, _)| request)
    }

    /// The URL and metadata a gRPC request connects with, as a key that
    /// changes when its variables point at another server. Generated
    /// variables such as `{{$guid}}` stay as written, since they differ on
    /// every use, unless a Before invoke script set them for the call.
    pub fn grpc_target_key(&self, request: &GrpcRequest) -> Option<Vec<String>> {
        // Servers may require credentials to answer reflection.
        let auth = self.effective_auth(&request.auth).texts();
        let texts = std::iter::once(request.url.as_str())
            .chain(Field::enabled(&request.metadata).flat_map(|(key, value)| [key, value]))
            .chain(auth.iter().map(String::as_str));
        let mut resolver = self.resolver();

        for text in texts.clone() {
            for reference in text.split("{{").skip(1) {
                if let Some((name, _)) = reference.split_once("}}")
                    && name.trim().starts_with('$')
                    && !self.generated.contains_key(name.trim())
                {
                    let name = name.trim().to_owned();
                    resolver.override_generated(name.clone(), format!("{{{{{name}}}}}"));
                }
            }
        }

        texts.map(|text| resolver.resolve(text).ok()).collect()
    }

    /// Resolve the URL, metadata and, with `message`, the message of a call
    /// together, so a generated value such as `{{$guid}}` is the same in each.
    /// Also returns the generated values, which the call's later scripts see.
    /// Later stream messages resolve with `resolve_text`.
    pub(crate) fn resolve_grpc(
        &self,
        request: &GrpcRequest,
        message: bool,
    ) -> Result<(GrpcRequest, HashMap<String, String>), String> {
        let mut resolver = self.resolver();
        let mut resolve = |text: &str| {
            resolver
                .resolve(text)
                .map_err(|error| describe_error(error, self.environment_error.as_deref()))
        };
        let mut request = request.clone();
        request.url = resolve(&request.url)?;
        request.metadata.retain(|field| field.enabled);

        for field in &mut request.metadata {
            field.key = resolve(&field.key)?;
            field.value = resolve(&field.value)?;
        }

        // Metadata that sends the credential itself takes precedence, so the
        // authorization's variables need no values.
        request.auth = self.effective_auth(&request.auth);
        let own_credential = request.auth.credential_name().is_some_and(|(_, name)| {
            Field::enabled(&request.metadata).any(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        });
        if own_credential {
            request.auth = Auth::None;
        }
        request.auth.resolve_with(&mut resolve)?;

        if message {
            request.message = resolve(&request.message)?;
        }

        Ok((request, resolver.generated_values().clone()))
    }

    /// A resolver that keeps the values a Before invoke script generated or set.
    fn resolver(&self) -> VariableResolver<'_> {
        let mut resolver = VariableResolver::new(&self.values);

        for (name, value) in &self.generated {
            resolver.override_generated(name.clone(), value.clone());
        }

        resolver
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
        request.headers.retain(|field| field.enabled);
        request.query.retain(|field| field.enabled);
        request.auth = self.effective_auth(&request.auth);
        let mut resolve = || {
            request.url = resolve_url(&request.url, &mut resolver)?;

            for field in request.headers.iter_mut().chain(request.query.iter_mut()) {
                field.key = resolver.resolve(&field.key)?;
                field.value = resolver.resolve(&field.value)?;
            }

            if crate::auth::sends_own_credential(
                &request.auth,
                &request.url,
                &request.query,
                &request.headers,
            ) {
                request.auth = Auth::None;
            }
            request.auth.resolve_with(|text| resolver.resolve(text))
        };

        resolve()
            .map(|()| request)
            .map_err(|error| describe_error(error, self.environment_error.as_deref()))
    }

    /// Resolve one outgoing WebSocket or gRPC stream message. Other generated
    /// values are new for each message.
    pub(crate) fn resolve_text(&self, text: &str) -> Result<String, String> {
        self.resolver()
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

/// The URL a request to `path` is sent to, without its query: its
/// `{{variables}}` and `:name` path variables filled as sending fills them,
/// with the values scripts `generated` or set for `{{$name}}`. Only the scheme,
/// host and path decide cookies, so a query variable that is not set yet does
/// not matter. None when another variable cannot be resolved.
pub(crate) fn sent_url(
    path: &str,
    path_variables: &[(String, String)],
    values: &HashMap<String, String>,
    generated: &BTreeMap<String, String>,
) -> Option<String> {
    let mut resolver = VariableResolver::new(values);
    resolver.limit_output(32 * 1024 * 1024);

    let overrides = generated
        .iter()
        .chain(values.iter().filter(|(name, _)| name.starts_with('$')));
    for (name, value) in overrides {
        resolver.override_generated(name.clone(), value.clone());
    }

    let path = &path[..query_start(path)];
    let url = resolve_url(path, &mut resolver).ok()?;
    let path = crate::request_url::fill_path_variables(&url, path_variables, |value| {
        resolver.resolve(value)
    })
    .ok()?;

    Some(
        HttpRequest {
            path,
            ..HttpRequest::default()
        }
        .prepare_for_send()
        .path,
    )
}

/// Where the URL's query or fragment starts, outside its `{{variables}}`.
fn query_start(url: &str) -> usize {
    let mut index = 0;

    while let Some(offset) = url[index..].find(['?', '#', '{']) {
        let at = index + offset;

        if url[at..].starts_with("{{") {
            let Some(end) = url[at + 2..].find("}}") else {
                return url.len();
            };
            index = at + 2 + end + 2;
        } else if url[at..].starts_with('{') {
            index = at + 1;
        } else {
            return at;
        }
    }

    url.len()
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
        // Fill path variables only where the resolved URL is sent, so a value
        // after a fragment is not resolved either.
        let path = resolve_url(&request.path, resolver)?;
        request.path =
            crate::request_url::fill_path_variables(&path, &request.path_variables, |value| {
                resolver.resolve(value)
            })?;
        request.headers.retain(|field| field.enabled);
        request.query.retain(|field| field.enabled);

        for field in request.headers.iter_mut().chain(request.query.iter_mut()) {
            field.key = resolver.resolve(&field.key)?;
            field.value = resolver.resolve(&field.value)?;
        }

        // A header or parameter that sends the credential itself takes
        // precedence, so the authorization's variables need no values.
        if crate::auth::sends_own_credential(
            &request.auth,
            &request.path,
            &request.query,
            &request.headers,
        ) {
            request.auth = Auth::None;
        }
        request.auth.resolve_with(|text| resolver.resolve(text))?;

        match &mut request.body {
            Some(Body::Raw { text, .. }) if body_changed || text.contains("{{") => {
                *text = resolver.resolve(text)?;
            }
            Some(Body::UrlEncoded { fields }) => {
                for (name, value) in fields {
                    *name = resolver.resolve(name)?;
                    *value = resolver.resolve(value)?;
                }
            }
            // A file is sent from the path as written.
            Some(Body::Multipart { parts }) => {
                for part in parts {
                    part.name = resolver.resolve(&part.name)?;
                    if !part.file {
                        part.value = resolver.resolve(&part.value)?;
                    }
                }
            }
            Some(Body::Raw { .. } | Body::Binary { .. }) | None => {}
        }

        Ok(request)
    }
}

/// Resolve a URL's `{{variables}}`. In the query, a variable's value is
/// encoded so it stays within its key or value; the text around it is sent as
/// written. A fragment is not sent, including one a variable introduces: the
/// rest of the path after that variable is dropped, its query is not.
fn resolve_url(text: &str, resolver: &mut VariableResolver<'_>) -> Result<String, VariableError> {
    let mut remaining = text.split('#').next().unwrap_or_default();
    let mut resolved = String::new();
    let mut in_query = false;

    loop {
        let start = remaining.find("{{").unwrap_or(remaining.len());
        let (literal, rest) = remaining.split_at(start);

        match literal.split_once('?') {
            Some((path, query)) if !in_query => {
                resolved.push_str(&resolver.resolve(path)?);
                // A whole-URL variable may have started the query already.
                resolved.push(if resolved.contains('?') { '&' } else { '?' });
                resolved.push_str(&resolver.resolve(query)?);
                in_query = true;
            }
            _ => resolved.push_str(&resolver.resolve(literal)?),
        }

        if rest.is_empty() {
            return Ok(resolved);
        }

        let end = rest[2..]
            .find("}}")
            .map(|end| end + 4)
            .ok_or(VariableError::Unclosed)?;
        let reference = &rest[..end];
        let value = resolver.resolve(reference)?;
        remaining = &rest[end..];

        if in_query && !reference.starts_with("{{!") {
            resolved.extend(form_urlencoded::byte_serialize(value.as_bytes()));
        } else if let Some((before_fragment, _)) = value.split_once('#') {
            resolved.push_str(before_fragment);

            // Unsent references are not resolved or validated.
            match remaining.find('?') {
                Some(query) if !in_query => remaining = &remaining[query..],
                _ => return Ok(resolved),
            }
        } else {
            resolved.push_str(&value);
        }
    }
}
