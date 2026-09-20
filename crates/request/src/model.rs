use serde::{Deserialize, Serialize};

/// Protocol-specific request data shared by collection files and editable drafts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Http(HttpRequest),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HttpRequest {
    pub method: Method,
    /// An absolute HTTP or HTTPS URL when executing the request.
    pub path: String,

    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<Vec<u8>>,
    #[serde(default)]
    pub query: Option<Vec<(String, String)>>,
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
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}
