use std::time::Duration;

use request::{
    Execution, HeaderMap, HeaderName, HttpMetrics, HttpResponse, Request, StatusCode, Version,
};
use serde::{Deserialize, Serialize};

/// The largest response body history keeps. Larger bodies are left out, so
/// history stays small on disk.
pub const BODY_LIMIT: usize = 1024 * 1024;

/// A request as it was sent, and what came back.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    /// As written, with its variables unresolved.
    pub request: Request,
    /// The HTTP response. gRPC calls and WebSocket connections keep only
    /// their request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Response>,
    /// Why the request failed after it was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Record {
    /// A request whose response, if any, is not kept.
    pub fn sent(request: impl Into<Request>) -> Self {
        Self {
            request: request.into(),
            response: None,
            error: None,
        }
    }

    /// The method or protocol shown before the address.
    pub fn label(&self) -> &'static str {
        self.request.label()
    }

    /// Where the request went, as written. A gRPC call adds its method.
    pub fn address(&self) -> String {
        match &self.request {
            Request::Grpc(request) if !request.method.trim().is_empty() => {
                format!("{}/{}", request.url.trim(), request.method.trim())
            }
            request => request.url().trim().to_owned(),
        }
    }
}

/// An HTTP response as history keeps it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response {
    pub status: u16,
    pub version: String,
    /// In the order received, including repeated headers.
    pub headers: Vec<(String, String)>,
    pub elapsed: Duration,
    pub metrics: HttpMetrics,
    /// The size of the received body, whether or not it is kept.
    pub body_size: usize,
    /// Missing when the body is larger than [`BODY_LIMIT`]. Stored in a file
    /// of its own.
    #[serde(skip)]
    pub body: Option<Vec<u8>>,
}

impl Response {
    pub fn new(execution: &Execution) -> Self {
        let request::Response::Http(response) = &execution.response;

        Self {
            status: response.status.as_u16(),
            version: format!("{:?}", response.version),
            headers: response
                .headers
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect(),
            elapsed: execution.elapsed,
            metrics: response.metrics,
            body_size: response.body.len(),
            body: (response.body.len() <= BODY_LIMIT).then(|| response.body.clone()),
        }
    }

    /// The response to show again. A body that was not kept is empty, and
    /// its scripts' results are not kept.
    pub fn into_execution(self) -> Execution {
        let mut headers = HeaderMap::new();
        for (name, value) in self.headers {
            if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), value.try_into()) {
                headers.append(name, value);
            }
        }

        Execution {
            response: request::Response::Http(HttpResponse {
                status: StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK),
                version: match self.version.as_str() {
                    "HTTP/0.9" => Version::HTTP_09,
                    "HTTP/1.0" => Version::HTTP_10,
                    "HTTP/2.0" => Version::HTTP_2,
                    "HTTP/3.0" => Version::HTTP_3,
                    _ => Version::HTTP_11,
                },
                headers,
                body: self.body.unwrap_or_default(),
                metrics: self.metrics,
            }),
            elapsed: self.elapsed,
            scripts: Vec::new(),
            sent: None,
        }
    }
}
