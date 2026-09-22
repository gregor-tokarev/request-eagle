use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Protocol-specific request data shared by collection files and editable drafts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Http(HttpRequest),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    /// An absolute HTTP or HTTPS URL when executing the request.
    pub path: String,

    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<Vec<u8>>,
    /// Structured form data takes precedence over the retained raw body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<FormBody>,
    #[serde(default)]
    pub query: Option<Vec<(String, String)>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "fields", rename_all = "snake_case")]
pub enum FormBody {
    UrlEncoded(Vec<(String, String)>),
    Multipart(Vec<MultipartField>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MultipartField {
    Text { name: String, value: String },
    File { name: String, path: PathBuf },
}

impl From<HttpRequest> for Request {
    fn from(request: HttpRequest) -> Self {
        Self::Http(request)
    }
}

impl From<&HttpRequest> for Request {
    fn from(request: &HttpRequest) -> Self {
        Self::Http(request.clone())
    }
}

impl From<&Request> for Request {
    fn from(request: &Request) -> Self {
        request.clone()
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
