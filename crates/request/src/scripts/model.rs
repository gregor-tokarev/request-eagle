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
