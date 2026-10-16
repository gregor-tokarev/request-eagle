//! The agent panel: a thread with a coding agent installed on the computer,
//! such as Claude Code, Codex or OpenCode. Messages can cite what is
//! selected in a request or its response.

mod actions;
mod entries;
mod option_menu;
mod panel;
mod prompt;
mod thread;
mod view;

#[cfg(test)]
mod prompt_tests;
#[cfg(test)]
mod thread_tests;

pub use actions::{OpenAgentSettings, ToggleAgentPanel, init};
pub use panel::{AgentPanel, AgentPanelEvent};
