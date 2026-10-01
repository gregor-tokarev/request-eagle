//! Request data and execution without application or UI state.
//!
//! `RequestExecutor::execute` takes an owned `HttpRequest` and its
//! `RequestVariables`, and returns a cancellable, sendable future. It can be
//! awaited on GPUI's background executor, smol, or Tokio.
//! `RequestExecutor::execute_streaming` also reports the events of a
//! `text/event-stream` response while they arrive, until it ends or is stopped.
//!
//! `WebSocketConnection::open` connects in the background and streams the
//! connection's events through a channel until it closes. `GrpcClient` loads
//! gRPC service definitions and starts calls whose events arrive on a channel.

mod error;
mod event_stream;
mod executor;
mod generated_headers;
mod grpc;
mod http;
mod model;
mod preferences;
mod proxy;
mod redirects;
mod response;
mod response_encoding;
mod scripts;
mod variables;
mod websocket;

#[cfg(test)]
mod event_stream_tests;

pub use scripts::{GrpcScripts, RequestScripts, ScriptLog, ScriptPhase, ScriptReport, ScriptTest};

pub use error::ExecutionError;
pub use event_stream::{
    Dispatch, EventStream, EventStreamUpdate, EventStreamUpdates, ServerSentEvent, StopEventStream,
};
pub use executor::RequestExecutor;
pub use generated_headers::generated_headers;
pub use grpc::{
    GrpcCall, GrpcClient, GrpcDefinition, GrpcError, GrpcEvent, GrpcEvents, GrpcMessage,
    GrpcMethod, GrpcRequest, GrpcService, GrpcSettings, GrpcStatus, MethodKind, PreparedCall,
    ServiceDefinition,
};
pub use http_client::http::{HeaderMap, HeaderName, StatusCode, Version};
pub use model::{HttpRequest, Method, Request, WebSocketRequest};
pub use preferences::{HttpVersion, RequestPreferences};
pub use proxy::{ProxyMode, ProxyPreferences, ProxyProtocol};
pub use response::{Execution, HttpMetrics, HttpResponse, Response};
pub use variables::RequestVariables;
pub use websocket::{
    WebSocketClose, WebSocketConnection, WebSocketEvent, WebSocketEventKind, WebSocketEvents,
    WebSocketHandshake, WebSocketMessage, websocket_handshake_headers,
};
