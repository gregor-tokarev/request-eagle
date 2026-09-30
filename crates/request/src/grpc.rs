//! gRPC calls with services known at runtime.
//!
//! `GrpcClient::load_definition` compiles a `.proto` file in-process, with its
//! imports resolved from the import paths and then the file's folder, or asks
//! the server with reflection (v1, falling back to v1alpha). Relative paths
//! resolve from the collection directory. `GrpcClient::invoke` starts a call of
//! any kind and returns a `GrpcCall`, which sends further messages and ends the
//! client stream, with a channel of `GrpcEvent`s: metadata, sent and received
//! messages as JSON, then the final status with trailers. Non-OK statuses are
//! completed calls; `Failed` means no status arrived, such as when the server is
//! unreachable. Dropping the call cancels it.
//!
//! `{{variables}}` resolve in the URL, metadata and each message. Certificate
//! checks, the response message limit and the timeout (for unary calls and
//! reflection) come from the request preferences unless the request's settings
//! override them. Calls run on the Tokio runtime shared with the HTTP client.
//! Scripts do not run for gRPC requests.

mod call;
mod client;
mod codec;
mod definition;
mod error;
mod example;
mod model;
mod reflection;
mod status;
mod transport;

pub use call::{GrpcCall, GrpcEvent, GrpcEvents, GrpcMessage};
pub use client::GrpcClient;
pub use definition::{GrpcMethod, GrpcService, MethodKind, ServiceDefinition};
pub use error::GrpcError;
pub use model::{GrpcDefinition, GrpcRequest, GrpcSettings};
pub use status::GrpcStatus;
