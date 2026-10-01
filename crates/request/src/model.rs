use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    /// An HTTP or HTTPS URL, which may contain `{{variables}}`.
    pub path: String,

    #[serde(default)]
    pub headers: Vec<Field>,
    #[serde(default, with = "body_text")]
    pub body: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<Field>,
    #[serde(default, skip_serializing_if = "crate::RequestScripts::is_empty")]
    pub scripts: crate::RequestScripts,
}

impl HttpRequest {
    /// Apply the request editor's URL and JSON defaults to a resolved snapshot.
    pub fn prepare_for_send(mut self) -> Self {
        self.path = self.path.trim().to_owned();
        if !self.path.is_empty() && !self.path.contains("://") && !self.path.starts_with("{{") {
            self.path = format!("https://{}", self.path);
        }

        if matches!(self.method, Method::Get | Method::Head) {
            self.body = None;
        } else if self.body.is_some()
            && !Field::enabled(&self.headers)
                .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            self.headers
                .push(Field::new("Content-Type", "application/json"));
        }

        self
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
    /// Saved with the request, so it can be sent again after reopening it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
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

/// Saves a body as text, which reads and diffs well in collection files.
/// A body that is not UTF-8 stays a list of bytes, as earlier versions saved
/// every body.
mod body_text {
    use super::*;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SavedBody {
        Text(String),
        Bytes(Vec<u8>),
    }

    pub(super) fn serialize<S: Serializer>(
        body: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match body.as_deref() {
            Some(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) => serializer.serialize_some(text),
                Err(_) => serializer.serialize_some(bytes),
            },
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        Ok(
            Option::<SavedBody>::deserialize(deserializer)?.map(|body| match body {
                SavedBody::Text(text) => text.into_bytes(),
                SavedBody::Bytes(bytes) => bytes,
            }),
        )
    }
}
