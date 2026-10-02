use gpui_kit::{App, Entity, actions};

use crate::logs;

actions!(request_eagle, [CheckForUpdates, Quit, ShowLogs]);

pub fn init(updater: Entity<updater::Updater>, cx: &mut App) {
    keybindings_service::register(
        CheckForUpdates,
        "Check for updates",
        "Look for a newer version of Request Eagle.",
        "Application",
        None,
        None,
        cx,
    )
    .expect("check for updates command should be valid");

    keybindings_service::register(
        ShowLogs,
        "Show logs",
        "Reveal the log file in Finder.",
        "Application",
        None,
        None,
        cx,
    )
    .expect("show logs command should be valid");

    keybindings_service::register(
        Quit,
        "Quit Request Eagle",
        "Close every window and quit the application.",
        "Application",
        Some("secondary-q"),
        None,
        cx,
    )
    .expect("default quit keybinding should be valid");

    cx.on_action(move |_: &CheckForUpdates, cx| {
        updater.update(cx, |updater, cx| updater.check(cx));
        cx.defer(|cx| cx.dispatch_action(&workspace::OpenGeneralSettings));
    })
    .on_action(|_: &ShowLogs, cx| {
        if let Some(path) = logs::file() {
            cx.reveal_path(&path);
        }
    })
    .on_action(|_: &Quit, cx| cx.quit());
}
