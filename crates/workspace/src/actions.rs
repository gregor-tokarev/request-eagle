use gpui_kit::{App, actions};

actions!(
    workspace,
    [ToggleLeftSidebar, OpenSettings, OpenGeneralSettings]
);

pub(crate) fn init(cx: &mut App) {
    keybindings_service::register(
        ToggleLeftSidebar,
        "Toggle sidebar",
        "Show or hide the collections sidebar.",
        "Workspace",
        Some("cmd-b"),
        None,
        cx,
    )
    .expect("default sidebar keybinding should be valid");

    keybindings_service::register(
        OpenSettings,
        "Open settings",
        "Open application settings.",
        "Settings",
        Some("cmd-,"),
        None,
        cx,
    )
    .expect("default settings keybinding should be valid");

    settings_ui::init(cx);
}
