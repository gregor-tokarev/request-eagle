//! The agent panel: a thread with a coding agent installed on the computer,
//! such as Claude Code, Codex or OpenCode. Messages can cite what is
//! selected in a request or its response, and mention saved requests, flows
//! and collections.

mod actions;
mod entries;
mod mention;
mod mention_menu;
mod option_menu;
mod panel;
mod prompt;
mod thread;
mod view;

#[cfg(test)]
mod mention_tests;
#[cfg(test)]
mod prompt_tests;
#[cfg(test)]
mod thread_tests;

pub use actions::{OpenAgentSettings, ToggleAgentPanel, init};
pub use mention::{Mention, MentionKind};
pub use panel::{AgentPanel, AgentPanelEvent};
