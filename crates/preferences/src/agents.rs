use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The coding agents the agent panel runs, and how.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct AgentPreferences {
    /// The agent the panel runs, such as `claude`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// Each agent's settings, by its ID. An agent without any has the
    /// defaults.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderPreferences>,
}

impl AgentPreferences {
    pub fn provider(&self, id: &str) -> ProviderPreferences {
        self.providers.get(id).cloned().unwrap_or_default()
    }
}

/// How the agent panel runs one agent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct ProviderPreferences {
    /// Whether the panel offers the agent.
    pub enabled: bool,
    /// The command that starts the agent's ACP server, such as an adapter
    /// installed globally. Empty starts the default one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// The folder the agent keeps its settings and sign-in in. Empty uses
    /// the agent's own.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub config_directory: String,
    /// The names of the environment variables the agent runs with. Their
    /// values are in the OS credential store.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<String>,
    /// Whether the agent asks before it runs commands and changes files.
    /// Otherwise it starts in its mode that does not ask, and the panel
    /// allows what it asks about anyway.
    pub ask_before_tools: bool,
    /// The models the agent offers, in the order its model menu lists them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelPreference>,
}

impl Default for ProviderPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            command: String::new(),
            config_directory: String::new(),
            environment: Vec::new(),
            ask_before_tools: false,
            models: Vec::new(),
        }
    }
}

/// A model an agent offers, as its model menu lists it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelPreference {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Left out of the model menu.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}
