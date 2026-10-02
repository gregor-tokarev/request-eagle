use gpui_kit::TestApp;

/// Startup disables a default shortcut that collides with an earlier one, so
/// every built-in shortcut must be free.
#[test]
fn default_shortcuts_do_not_conflict() {
    let mut app = TestApp::new();

    app.update(|cx| {
        crate::actions::init(cx);

        for command in keybindings_service::commands(cx) {
            assert_eq!(command.binding_error, None, "{}", command.label);
            assert_eq!(
                command.binding, command.default_binding,
                "{}",
                command.label
            );
        }
    });
}
