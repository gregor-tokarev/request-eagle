use gpui_kit::{App, Entity, actions};

actions!(request_eagle, [CheckForUpdates, Quit]);

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
    .on_action(|_: &Quit, cx| cx.quit());
}
