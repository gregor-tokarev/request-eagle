//! Request data and execution without application or UI state.
//!
//! `RequestExecutor::execute` takes an owned `HttpRequest` and its
//! `RequestVariables`, and returns a cancellable, sendable future. It can be
//! awaited on GPUI's background executor, smol, or Tokio.

mod error;
mod executor;
mod generated_headers;
mod http;
mod model;
mod preferences;
mod proxy;
mod redirects;
mod response;
mod response_encoding;
mod scripts;
mod variables;

pub use scripts::{RequestScripts, ScriptLog, ScriptPhase, ScriptReport, ScriptTest};

pub use error::ExecutionError;
pub use executor::RequestExecutor;
pub use generated_headers::generated_headers;
pub use http_client::http::{HeaderMap, HeaderName, StatusCode, Version};
pub use model::{HttpRequest, Method, Request};
pub use preferences::{HttpVersion, RequestPreferences};
pub use proxy::{ProxyMode, ProxyPreferences, ProxyProtocol};
pub use response::{Execution, HttpMetrics, HttpResponse, Response};
pub use variables::RequestVariables;
