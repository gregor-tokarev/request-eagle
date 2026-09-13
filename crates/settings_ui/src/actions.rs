use gpui_kit::{App, actions};

// Keep the existing action identifier so saved shortcut overrides remain valid.
actions!(workspace, [CloseSettings]);

pub fn init(cx: &mut App) {
    keybindings_service::register(
        CloseSettings,
        "Close settings",
        "Return to your workspace.",
        "Settings",
        Some("escape"),
        Some("Settings"),
        cx,
    )
    .expect("default close settings keybinding should be valid");
}
