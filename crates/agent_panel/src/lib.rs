//! The agent panel: a thread with a coding agent installed on the computer,
//! such as Claude Code, Codex or OpenCode, which it runs over the Agent
//! Client Protocol. Messages can cite what is selected in a request or its
//! response.

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

pub use panel::{AgentPanel, AgentPanelEvent};

gpui_kit::actions!(agent, [ToggleAgentPanel]);

pub fn init(cx: &mut gpui_kit::App) {
    keybindings_service::register(
        ToggleAgentPanel,
        "Toggle agent panel",
        "Show or hide the agent panel.",
        "Agent",
        Some("secondary-shift-i"),
        Some("Workspace"),
        cx,
    )
    .expect("default agent panel keybinding should be valid");

    keybindings_service::register(
        tab_ui::AddToAgent,
        "Add to agent",
        "Cite the selection, or the field with focus, in the agent's message.",
        "Agent",
        Some("secondary-i"),
        Some("Workspace"),
        cx,
    )
    .expect("default add to agent keybinding should be valid");
}
