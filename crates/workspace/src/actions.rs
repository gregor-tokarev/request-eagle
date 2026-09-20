use gpui_kit::{Action, App, actions};

actions!(
    workspace,
    [
        ToggleLeftSidebar,
        OpenSettings,
        OpenGeneralSettings,
        NewTab,
        CloseTab,
        PreviousTab,
        NextTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
    ]
);

fn register_tab_action<A: Action>(
    action: A,
    label: &'static str,
    description: &'static str,
    shortcut: &str,
    cx: &mut App,
) {
    keybindings_service::register(
        action,
        label,
        description,
        "Tabs",
        Some(shortcut),
        Some("Workspace"),
        cx,
    )
    .expect("default tab keybinding should be valid");
}

pub(crate) fn init(cx: &mut App) {
    register_tab_action(NewTab, "New tab", "Open an empty tab.", "secondary-t", cx);
    register_tab_action(
        CloseTab,
        "Close tab",
        "Close the active tab.",
        "secondary-w",
        cx,
    );
    // GPUI folds Shift into punctuation on macOS and Linux (Shift+[ becomes {).
    register_tab_action(
        PreviousTab,
        "Previous tab",
        "Switch to the previous tab.",
        "secondary-{",
        cx,
    );
    register_tab_action(
        NextTab,
        "Next tab",
        "Switch to the next tab.",
        "secondary-}",
        cx,
    );
    register_tab_action(
        SelectTab1,
        "Select tab 1",
        "Switch to the first tab.",
        "secondary-1",
        cx,
    );
    register_tab_action(
        SelectTab2,
        "Select tab 2",
        "Switch to the second tab.",
        "secondary-2",
        cx,
    );
    register_tab_action(
        SelectTab3,
        "Select tab 3",
        "Switch to the third tab.",
        "secondary-3",
        cx,
    );
    register_tab_action(
        SelectTab4,
        "Select tab 4",
        "Switch to the fourth tab.",
        "secondary-4",
        cx,
    );
    register_tab_action(
        SelectTab5,
        "Select tab 5",
        "Switch to the fifth tab.",
        "secondary-5",
        cx,
    );
    register_tab_action(
        SelectTab6,
        "Select tab 6",
        "Switch to the sixth tab.",
        "secondary-6",
        cx,
    );
    register_tab_action(
        SelectTab7,
        "Select tab 7",
        "Switch to the seventh tab.",
        "secondary-7",
        cx,
    );
    register_tab_action(
        SelectTab8,
        "Select tab 8",
        "Switch to the eighth tab.",
        "secondary-8",
        cx,
    );
    register_tab_action(
        SelectLastTab,
        "Select last tab",
        "Switch to the last tab.",
        "secondary-9",
        cx,
    );

    keybindings_service::register(
        ToggleLeftSidebar,
        "Toggle sidebar",
        "Show or hide the collections sidebar.",
        "Workspace",
        Some("secondary-b"),
        None,
        cx,
    )
    .expect("default sidebar keybinding should be valid");

    keybindings_service::register(
        OpenSettings,
        "Open settings",
        "Open application settings.",
        "Settings",
        Some("secondary-,"),
        None,
        cx,
    )
    .expect("default settings keybinding should be valid");

    settings_ui::init(cx);
}
