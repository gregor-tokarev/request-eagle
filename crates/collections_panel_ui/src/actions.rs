use gpui_kit::{App, actions};

actions!(collections_panel, [RenameItem, DeleteItem]);

/// Sidebar shortcuts apply to the selected row, but not while typing in a
/// rename editor inside the tree.
const CONTEXT: &str = "CollectionsSidebar && !Input";

pub fn init(cx: &mut App) {
    keybindings_service::register(
        RenameItem,
        "Rename sidebar item",
        "Rename the selected collection, folder, or request.",
        "Collections",
        Some("f2"),
        Some(CONTEXT),
        cx,
    )
    .expect("default rename keybinding should be valid");

    keybindings_service::register(
        DeleteItem,
        "Delete sidebar item",
        "Delete the selected collection, folder, or request after confirmation.",
        "Collections",
        Some("backspace"),
        Some(CONTEXT),
        cx,
    )
    .expect("default delete keybinding should be valid");
}
