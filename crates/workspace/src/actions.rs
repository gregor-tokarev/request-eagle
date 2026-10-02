use gpui_kit::{Action, App, actions};

pub use tab_ui::SendRequest;

actions!(
    workspace,
    [
        ToggleLeftSidebar,
        ToggleCommandPalette,
        FocusSidebarSearch,
        OpenSettings,
        OpenGeneralSettings,
        OpenCookies,
        OpenEnvironmentSelector,
        NewCollection,
        ImportCollection,
        SaveRequest,
        FocusUrl,
        // Keep the existing action name so saved keyboard shortcuts continue
        // to work. gRPC requests are copied as grpcurl.
        CopyAsCurl,
        NewTab,
        NewGrpcTab,
        NewWebSocketTab,
        CloseTab,
        RenameTab,
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
    keybindings_service::register(
        SendRequest,
        "Send request",
        "Send the request in the active tab.",
        "Requests",
        Some("secondary-enter"),
        // Input has its own Command+Enter action. Match at the input as well
        // as the workspace so it cannot consume the request shortcut.
        Some("Workspace || (Workspace > Input)"),
        cx,
    )
    .expect("default send request keybinding should be valid");

    keybindings_service::register(
        SaveRequest,
        "Save request",
        "Save changes to the request in the active tab.",
        "Requests",
        Some("secondary-s"),
        Some("Workspace || (Workspace > Input)"),
        cx,
    )
    .expect("default save request keybinding should be valid");

    keybindings_service::register(
        FocusUrl,
        "Focus URL bar",
        "Select the URL of the request in the active tab.",
        "Requests",
        Some("secondary-l"),
        Some("Workspace"),
        cx,
    )
    .expect("default focus URL keybinding should be valid");

    keybindings_service::register(
        CopyAsCurl,
        "Copy as cURL or grpcurl",
        "Copy the request in the active tab as a cURL command, or as a grpcurl command for gRPC.",
        "Requests",
        Some("secondary-shift-c"),
        Some("Workspace"),
        cx,
    )
    .expect("default copy as command keybinding should be valid");

    register_tab_action(
        NewTab,
        "New tab",
        "Open an empty GET request.",
        "secondary-t",
        cx,
    );
    keybindings_service::register(
        NewGrpcTab,
        "New gRPC tab",
        "Open an empty gRPC request.",
        "Tabs",
        Some("secondary-alt-g"),
        Some("Workspace"),
        cx,
    )
    .expect("default gRPC tab keybinding should be valid");
    keybindings_service::register(
        NewWebSocketTab,
        "New WebSocket tab",
        "Open an empty WebSocket request.",
        "Tabs",
        Some("secondary-alt-w"),
        Some("Workspace"),
        cx,
    )
    .expect("default WebSocket tab keybinding should be valid");
    register_tab_action(
        CloseTab,
        "Close tab",
        "Close the active tab.",
        "secondary-w",
        cx,
    );
    register_tab_action(
        RenameTab,
        "Rename tab",
        "Rename the request in the active tab.",
        "f2",
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
        ToggleCommandPalette,
        "Command palette",
        "Run a command, or find a request, collection, or environment by name.",
        "Workspace",
        Some("secondary-k"),
        None,
        cx,
    )
    .expect("default command palette keybinding should be valid");

    keybindings_service::register(
        FocusSidebarSearch,
        "Search collections",
        "Focus the sidebar search when focus is outside the response.",
        "Workspace",
        Some("secondary-f"),
        // Match input contexts too, while preserving response editor search.
        Some("(Workspace || (Workspace > Input)) && !Response"),
        cx,
    )
    .expect("default sidebar search keybinding should be valid");

    keybindings_service::register(
        OpenCookies,
        "Open cookies",
        "Show the cookies that requests store and send, and delete them.",
        "Requests",
        Some("secondary-shift-k"),
        None,
        cx,
    )
    .expect("default cookies keybinding should be valid");

    keybindings_service::register(
        OpenEnvironmentSelector,
        "Open environment selector",
        "Choose the environment that requests use.",
        "Environments",
        Some("secondary-shift-e"),
        Some("Workspace"),
        cx,
    )
    .expect("default environment selector keybinding should be valid");

    keybindings_service::register(
        NewCollection,
        "New collection",
        "Create a collection and name it in the sidebar.",
        "Collections",
        Some("secondary-shift-n"),
        Some("Workspace"),
        cx,
    )
    .expect("default new collection keybinding should be valid");

    keybindings_service::register(
        ImportCollection,
        "Import collection",
        "Import a cURL command, Postman collections and environments, or an OpenAPI specification.",
        "Collections",
        Some("secondary-o"),
        Some("Workspace"),
        cx,
    )
    .expect("default import collection keybinding should be valid");

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

    collections_panel_ui::init(cx);
    settings_ui::init(cx);
}
