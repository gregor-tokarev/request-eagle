//! The agent panel: a thread with a coding agent installed on the computer,
//! such as Claude Code, Codex or OpenCode, which it runs over the Agent
//! Client Protocol. Messages can cite what is selected in a request or its
//! response.

mod actions;
mod agents;
mod connection;
mod entries;
mod panel;
mod prompt;
mod shell_path;
mod thread;
mod view;

#[cfg(test)]
mod agents_tests;
#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod prompt_tests;
#[cfg(test)]
mod shell_path_tests;
#[cfg(test)]
mod thread_tests;

pub use actions::{ToggleAgentPanel, init};
pub use panel::{AgentPanel, AgentPanelEvent};
