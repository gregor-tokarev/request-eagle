gpui_kit::actions!(
    agent,
    [ToggleAgentPanel, OpenAgentSettings, ChangeModel, NewThread]
);

/// Register the panel's shortcuts.
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
        Some("secondary-l"),
        Some("Workspace"),
        cx,
    )
    .expect("default add to agent keybinding should be valid");

    keybindings_service::register(
        OpenAgentSettings,
        "Open agent settings",
        "Choose the coding agents the agent panel runs, and how.",
        "Agent",
        None,
        Some("Workspace"),
        cx,
    )
    .expect("agent settings command should be valid");

    keybindings_service::register(
        ChangeModel,
        "Change model",
        "Open the agent's model menu.",
        "Agent",
        Some("secondary-/"),
        Some("AgentPanel"),
        cx,
    )
    .expect("default change model keybinding should be valid");

    keybindings_service::register(
        NewThread,
        "New thread",
        "Start a new thread with the agent from its message box.",
        "Agent",
        Some("ctrl-n"),
        Some("AgentComposer"),
        cx,
    )
    .expect("default new thread keybinding should be valid");
}
