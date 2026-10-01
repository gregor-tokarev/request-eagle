use gpui_kit::{App, Menu, MenuItem};

use crate::actions::{CheckForUpdates, Quit, ShowLogs};

pub fn use_menus(_: &mut App) -> Vec<Menu> {
    vec![Menu {
        name: "Request Eagle".into(),
        disabled: false,
        items: vec![
            MenuItem::action("About Request Eagle", workspace::OpenGeneralSettings),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::action("Show Logs", ShowLogs),
            MenuItem::separator(),
            MenuItem::action("Settings…", workspace::OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Request Eagle", Quit),
        ],
    }]
}

pub fn init(cx: &mut App) {
    let menus = use_menus(cx);
    cx.set_menus(menus);

    // AppKit's menu key equivalents are a snapshot of the keymap.
    cx.observe_global::<keybindings_service::KeybindingsService>(|cx| {
        let menus = use_menus(cx);
        cx.set_menus(menus);
    })
    .detach();
}
