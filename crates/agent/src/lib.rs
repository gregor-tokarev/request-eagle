//! Coding agents installed on the computer, such as Claude Code, Codex and
//! OpenCode, run over the Agent Client Protocol.

mod agents;
mod check;
mod connection;
mod options;
mod shell_path;

#[cfg(test)]
mod agents_tests;
#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod options_tests;
#[cfg(test)]
mod shell_path_tests;

pub use agent_client_protocol as acp;
pub use agents::{AGENTS, Agent, find_program};
pub use check::{Check, Status, check};
pub use connection::{Command, Connection, Event, Failure, Launch};
pub use options::{bypass_mode, choices, is_bypass, merge_models, offered_models, prompting_mode};
pub use shell_path::agent_path;
