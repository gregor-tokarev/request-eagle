use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RequestScripts {
    #[serde(default)]
    pub pre_request: String,
    #[serde(default)]
    pub post_response: String,
}

impl RequestScripts {
    pub fn is_empty(&self) -> bool {
        self.pre_request.is_empty() && self.post_response.is_empty()
    }
}

/// A gRPC request's scripts, named after the moment of the call they run at.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct GrpcScripts {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_invoke: String,
    /// Runs once for every message the server sends.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on_message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_response: String,
}

impl GrpcScripts {
    pub fn is_empty(&self) -> bool {
        self.before_invoke.is_empty()
            && self.on_message.is_empty()
            && self.after_response.is_empty()
    }
}

/// What `pm.info` tells scripts about the request and, in a Collection
/// Runner, the iteration it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionInfo {
    pub request_name: String,
    pub request_id: String,
    /// Counting from 0.
    pub iteration: usize,
    pub iteration_count: usize,
}

impl Default for ExecutionInfo {
    /// A request sent on its own runs once.
    fn default() -> Self {
        Self {
            request_name: String::new(),
            request_id: String::new(),
            iteration: 0,
            iteration_count: 1,
        }
    }
}

impl ExecutionInfo {
    pub(super) fn input(&self, event: &str) -> serde_json::Value {
        serde_json::json!({
            "eventName": event,
            "iteration": self.iteration,
            "iterationCount": self.iteration_count,
            "requestName": self.request_name,
            "requestId": self.request_id,
        })
    }
}

/// `pm.variables` values that requests pass on to the next one, as the
/// requests of a Collection Runner's run do. Clones share the values.
#[derive(Clone, Debug, Default)]
pub struct LocalVariables(Arc<Mutex<BTreeMap<String, String>>>);

impl LocalVariables {
    pub fn get(&self) -> BTreeMap<String, String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(super) fn set(&self, values: BTreeMap<String, String>) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = values;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptPhase {
    PreRequest,
    PostResponse,
    BeforeInvoke,
    OnMessage,
    AfterResponse,
}

impl ScriptPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::PreRequest => "Pre-request",
            Self::PostResponse => "Post-response",
            Self::BeforeInvoke => "Before invoke",
            Self::OnMessage => "On message",
            Self::AfterResponse => "After response",
        }
    }

    /// Whether the phase runs during a gRPC call rather than an HTTP request.
    pub fn is_grpc(self) -> bool {
        matches!(
            self,
            Self::BeforeInvoke | Self::OnMessage | Self::AfterResponse
        )
    }
}

#[derive(Clone, Debug)]
pub struct ScriptReport {
    pub phase: ScriptPhase,
    /// Whether the collection's script produced this report, not the request's.
    pub collection: bool,
    /// The received message an On message report belongs to, counting from 1.
    pub message: Option<usize>,
    pub tests: Vec<ScriptTest>,
    pub logs: Vec<ScriptLog>,
    pub error: Option<String>,
    /// What the script chose with `pm.execution.setNextRequest`. Only the
    /// Collection Runner follows it.
    pub next_request: Option<NextRequest>,
}

/// The request a Collection Runner sends after this one, as a script set it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextRequest {
    /// `setNextRequest(null)` ends the iteration after this request.
    Stop,
    /// A request's name or ID.
    Request(String),
}

impl ScriptReport {
    pub fn label(&self) -> String {
        if self.collection {
            format!("Collection {}", self.phase.label().to_lowercase())
        } else if let Some(message) = self.message {
            format!("{} {message}", self.phase.label())
        } else {
            self.phase.label().to_owned()
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScriptTest {
    pub name: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ScriptLog {
    pub level: String,
    pub message: String,
}
