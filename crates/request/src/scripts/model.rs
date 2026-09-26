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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptPhase {
    PreRequest,
    PostResponse,
}

impl ScriptPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::PreRequest => "Pre-request",
            Self::PostResponse => "Post-response",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScriptReport {
    pub phase: ScriptPhase,
    pub tests: Vec<ScriptTest>,
    pub logs: Vec<ScriptLog>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScriptTest {
    pub name: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScriptLog {
    pub level: String,
    pub message: String,
}
