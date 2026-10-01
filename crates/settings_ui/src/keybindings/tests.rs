use super::search::matches_search;
use gpui_kit::{Action, actions};
use keybindings_service as keybindings;

actions!(settings_tests, [OpenSettings, ToggleLeftSidebar]);

#[test]
fn search_accepts_names_symbols_and_modifier_aliases() {
    let mut app = gpui_kit::TestApp::new();
    app.update(init_commands);

    app.read(|cx| {
        let commands = keybindings::commands(cx);
        let found = commands
            .iter()
            .filter(|c| matches_search(c, "sidebar"))
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, ToggleLeftSidebar::name_for_type());

        for query in ["⌘B", "Command+B", "cmd-b", "collections"] {
            assert!(matches_search(found[0], query), "{query}");
        }

        assert!(!matches_search(found[0], "sidebar no-such-command"));
    });
}

fn init_commands(cx: &mut gpui_kit::App) {
    crate::init(cx);
    keybindings::register(
        ToggleLeftSidebar,
        "Toggle sidebar",
        "Show or hide the collections sidebar.",
        "Workspace",
        Some("cmd-b"),
        None,
        cx,
    )
    .unwrap();
    keybindings::register(
        OpenSettings,
        "Open settings",
        "Open application settings.",
        "Settings",
        Some("cmd-,"),
        None,
        cx,
    )
    .unwrap();
}
