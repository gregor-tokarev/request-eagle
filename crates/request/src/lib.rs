//! Request data and execution without application or UI state.
//!
//! `RequestExecutor::execute` snapshots a saved `Request` or a draft's
//! `HttpRequest` and returns a cancellable, sendable future. It can be awaited
//! on GPUI's background executor, smol, or Tokio.
//!
//! Add future protocols as `Request` / `Response` variants with their own
//! execution modules. Streaming protocols can return a session in their
//! response variant; they do not have to use the buffered HTTP response.

mod authentication;
mod error;
mod executor;
mod generated_headers;
mod http;
mod model;
mod preferences;
mod proxy;
mod redirects;
mod request_body;
mod response;
mod response_encoding;
mod variables;

#[cfg(test)]
mod request_body_tests;

pub use authentication::{ApiKeyLocation, Authentication};
pub use error::{ExecutionError, HttpError};
pub use executor::RequestExecutor;
pub use generated_headers::generated_headers;
pub use http_client::http::{HeaderMap, HeaderName, StatusCode, Version};
pub use model::{FormBody, HttpRequest, Method, MultipartField, Request};
pub use preferences::{HttpVersion, RequestPreferences};
pub use proxy::{ProxyMode, ProxyPreferences, ProxyProtocol};
pub use response::{Execution, HttpMetrics, HttpResponse, Response};
pub use variables::{VariableError, resolve_variables};
