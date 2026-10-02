use std::path::Path;

use serde::{Deserialize, Serialize};

/// Protocol-specific request data shared by collection files and editable drafts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Http(HttpRequest),
    Grpc(crate::GrpcRequest),
    #[serde(rename = "websocket")]
    WebSocket(WebSocketRequest),
}

impl Request {
    /// The label shown before the request's name: its HTTP method or protocol.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Http(request) => request.method.as_str(),
            Self::Grpc(_) => "gRPC",
            Self::WebSocket(_) => "WS",
        }
    }

    /// The address the request is sent to, as written.
    pub fn url(&self) -> &str {
        match self {
            Self::Http(request) => &request.path,
            Self::Grpc(request) => &request.url,
            Self::WebSocket(request) => &request.url,
        }
    }

    /// Resolve file paths relative to `collection`, so a copy of the
    /// request kept outside it still finds its files.
    pub fn resolved_from(&self, collection: &Path) -> Self {
        match self {
            Self::Http(request) => Self::Http(HttpRequest {
                body: request
                    .body
                    .as_ref()
                    .map(|body| body.resolved_from(collection)),
                ..request.clone()
            }),
            Self::Grpc(request) => Self::Grpc(crate::GrpcRequest {
                definition: request.definition.resolved_from(collection),
                ..request.clone()
            }),
            Self::WebSocket(request) => Self::WebSocket(request.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    /// An HTTP or HTTPS URL, which may contain `{{variables}}`.
    pub path: String,

    #[serde(default)]
    pub headers: Vec<Field>,
    #[serde(
        default,
        deserialize_with = "crate::body::stored",
        skip_serializing_if = "Option::is_none"
    )]
    pub body: Option<crate::Body>,
    /// Sent after the URL's own query, except rows that are switched off.
    /// The app keeps the parameters it sends in the URL and only the
    /// switched-off ones here; see `inline_query`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<Field>,
    /// Values for the `:name` segments of the URL's path. A variable without
    /// a value is sent as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_variables: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "crate::Auth::is_inherit")]
    pub auth: crate::Auth,
    #[serde(default, skip_serializing_if = "crate::RequestScripts::is_empty")]
    pub scripts: crate::RequestScripts,
    #[serde(default, skip_serializing_if = "HttpSettings::is_default")]
    pub settings: HttpSettings,
}

/// Per-request options. Each unset option follows the request preferences.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct HttpSettings {
    /// Deadline through the complete response body. Zero disables it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_certificates: Option<bool>,
}

impl HttpSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// How many options differ from the request preferences.
    pub fn overrides(&self) -> usize {
        usize::from(self.timeout_ms.is_some())
            + usize::from(self.follow_redirects.is_some())
            + usize::from(self.verify_certificates.is_some())
    }
}

impl HttpRequest {
    /// Move the switched-on rows of `query` into the URL, where the Params
    /// table edits them. Sending the request is unchanged.
    pub fn inline_query(&mut self) {
        let sent = Field::pairs(&self.query);
        if !sent.is_empty() {
            self.path = crate::request_url::append_encoded_query(&self.path, &sent);
            self.query.retain(|field| !field.enabled);
        }
    }

    /// Apply the request editor's URL default to a resolved snapshot, and
    /// leave out the body of a method that sends none.
    pub fn prepare_for_send(mut self) -> Self {
        self.path = self.path.trim().to_owned();
        if !self.path.is_empty() && !self.path.contains("://") && !self.path.starts_with("{{") {
            self.path = format!("https://{}", self.path);
        }

        if matches!(self.method, Method::Get | Method::Head) {
            self.body = None;
        }

        self
    }

    /// The bytes of the resolved body, with its files read. Adds the
    /// `Content-Type` the body needs unless the request sets one; a
    /// multipart type written without its boundary gets it. Empty raw text
    /// sends no body.
    pub(crate) fn encode_body(&mut self) -> Result<Option<Vec<u8>>, crate::ExecutionError> {
        let Some(body) = &mut self.body else {
            return Ok(None);
        };
        if matches!(body, crate::Body::Raw { text, .. } if text.is_empty()) {
            return Ok(None);
        }
        let (bytes, content_type) = body.encode()?;

        let own = self
            .headers
            .iter_mut()
            .find(|field| field.enabled && field.key.eq_ignore_ascii_case("content-type"));
        let Some(Field { value, .. }) = own else {
            self.headers.push(Field::new("Content-Type", content_type));
            return Ok(Some(bytes));
        };

        let lowercase = value.to_ascii_lowercase();
        if lowercase.trim_start().starts_with("multipart/")
            && !lowercase.contains("boundary=")
            && let Some((_, boundary)) = content_type.split_once("; boundary=")
        {
            value.push_str("; boundary=");
            value.push_str(boundary);
        }

        Ok(Some(bytes))
    }
}

impl From<HttpRequest> for Request {
    fn from(request: HttpRequest) -> Self {
        Self::Http(request)
    }
}

impl From<crate::GrpcRequest> for Request {
    fn from(request: crate::GrpcRequest) -> Self {
        Self::Grpc(request)
    }
}

/// A WebSocket connection's address and handshake, and the message being composed.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebSocketRequest {
    /// A ws or wss URL, which may contain `{{variables}}`.
    pub url: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<Field>,
    #[serde(default, skip_serializing_if = "crate::Auth::is_inherit")]
    pub auth: crate::Auth,
    /// Saved with the request, so it can be sent again after reopening it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "WebSocketSettings::is_default")]
    pub settings: WebSocketSettings,
}

/// Per-connection options. Each unset option follows the request preferences.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct WebSocketSettings {
    /// Deadline for the handshake. Zero disables it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_certificates: Option<bool>,
}

impl WebSocketSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// How many options differ from the request preferences.
    pub fn overrides(&self) -> usize {
        usize::from(self.timeout_ms.is_some()) + usize::from(self.verify_certificates.is_some())
    }
}

impl From<WebSocketRequest> for Request {
    fn from(request: WebSocketRequest) -> Self {
        Self::WebSocket(request)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Head,
    Options,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Delete => "DELETE",
        }
    }
}

/// A row of a request's parameters, headers or gRPC metadata. A row that is
/// switched off stays with the request but is not sent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(from = "SavedField", into = "SavedField")]
pub struct Field {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub description: String,
}

impl Field {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
            description: String::new(),
        }
    }

    /// The key and value of each row that is sent.
    pub fn enabled(fields: &[Self]) -> impl Iterator<Item = (&str, &str)> + Clone {
        fields
            .iter()
            .filter(|field| field.enabled)
            .map(|field| (field.key.as_str(), field.value.as_str()))
    }

    /// The rows that are sent, as the pairs a URL's query or a form holds.
    pub fn pairs(fields: &[Self]) -> Vec<(String, String)> {
        Self::enabled(fields)
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }
}

impl<K: Into<String>, V: Into<String>> From<(K, V)> for Field {
    fn from((key, value): (K, V)) -> Self {
        Self::new(key, value)
    }
}

/// Enabled rows without a description keep the `[key, value]` form that
/// earlier versions wrote.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum SavedField {
    Pair(String, String),
    Row {
        key: String,
        value: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        disabled: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
    },
}

impl From<SavedField> for Field {
    fn from(field: SavedField) -> Self {
        match field {
            SavedField::Pair(key, value) => Self::new(key, value),
            SavedField::Row {
                key,
                value,
                disabled,
                description,
            } => Self {
                key,
                value,
                enabled: !disabled,
                description,
            },
        }
    }
}

impl From<Field> for SavedField {
    fn from(field: Field) -> Self {
        if field.enabled && field.description.is_empty() {
            Self::Pair(field.key, field.value)
        } else {
            Self::Row {
                key: field.key,
                value: field.value,
                disabled: !field.enabled,
                description: field.description,
            }
        }
    }
}
