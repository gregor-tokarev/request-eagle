use serde::{Deserialize, Serialize};

/// Protocol-specific request data shared by collection files and editable drafts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Http(HttpRequest),
    Grpc(crate::GrpcRequest),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    /// An HTTP or HTTPS URL, which may contain `{{variables}}`.
    pub path: String,

    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<(String, String)>,
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
            && !self
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            self.headers
                .push(("Content-Type".into(), "application/json".into()));
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
