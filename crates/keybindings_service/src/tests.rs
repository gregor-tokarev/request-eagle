use gpui_kit::{KeyBinding, KeyContext, Keystroke, TestApp, actions};

use super::*;
use crate::Binding;
use gpui_kit::{Action, App};
use std::collections::BTreeMap;

actions!(keybindings_service_tests, [FirstAction, OtherAction]);

#[test]
fn replaces_a_binding_at_runtime() {
    let mut app = TestApp::new();

    app.update(|cx| set_binding("cmd-q", FirstAction, None, cx).unwrap());
    assert_eq!(
        actions_for("cmd-q", &app),
        vec!["keybindings_service_tests::FirstAction"]
    );

    app.update(|cx| set_binding("cmd-w", FirstAction, None, cx).unwrap());

    assert!(actions_for("cmd-q", &app).is_empty());
    assert_eq!(
        actions_for("cmd-w", &app),
        vec!["keybindings_service_tests::FirstAction"]
    );
    app.read_global::<KeybindingsService, _>(|_, cx| {
        assert_eq!(
            binding_for::<FirstAction>(cx),
            Some(&Binding {
                keystrokes: Keystroke::parse("cmd-w").unwrap().unparse(),
                context: None,
            })
        );
    });
}

#[test]
fn keeps_bindings_that_the_service_does_not_own() {
    let mut app = TestApp::new();

    app.update(|cx| {
        cx.bind_keys([KeyBinding::new("cmd-o", OtherAction, None)]);
        set_binding("cmd-q", FirstAction, None, cx).unwrap();
        set_binding("cmd-w", FirstAction, None, cx).unwrap();
    });

    assert_eq!(
        actions_for("cmd-o", &app),
        vec!["keybindings_service_tests::OtherAction"]
    );
}

#[test]
fn invalid_replacement_leaves_the_current_binding_active() {
    let mut app = TestApp::new();

    app.update(|cx| {
        set_binding("cmd-q", FirstAction, None, cx).unwrap();
        assert!(set_binding("cmd-a-b", FirstAction, None, cx).is_err());
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec!["keybindings_service_tests::FirstAction"]
    );
}

fn register_commands(cx: &mut App) {
    register(
        FirstAction,
        "First command",
        "First description",
        "Application",
        Some("cmd-q"),
        None,
        cx,
    )
    .unwrap();

    register(
        OtherAction,
        "Other command",
        "Other description",
        "Application",
        Some("cmd-b"),
        None,
        cx,
    )
    .unwrap();
}

#[test]
fn overrides_and_removed_shortcuts_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keybindings.json");
    let mut app = TestApp::new();

    app.update(|cx| {
        load_overrides(path.clone(), cx).unwrap();
        register_commands(cx);

        set_override(FirstAction::name_for_type(), Some("shift-cmd-k"), cx).unwrap();
        set_override(OtherAction::name_for_type(), None, cx).unwrap();
    });

    assert!(actions_for("cmd-q", &app).is_empty());
    assert!(actions_for("cmd-b", &app).is_empty());
    assert_eq!(
        actions_for("cmd-shift-k", &app),
        vec![FirstAction::name_for_type()]
    );

    let mut restarted = TestApp::new();
    restarted.update(|cx| {
        load_overrides(path.clone(), cx).unwrap();
        register_commands(cx);

        assert_eq!(commands(cx).len(), 2);
        assert!(commands(cx).iter().all(Command::is_modified));
    });

    assert_eq!(
        actions_for("cmd-shift-k", &restarted),
        vec![FirstAction::name_for_type()]
    );
    assert!(actions_for("cmd-b", &restarted).is_empty());

    restarted.update(|cx| reset_all(cx).unwrap());

    assert_eq!(
        actions_for("cmd-q", &restarted),
        vec![FirstAction::name_for_type()]
    );
    assert_eq!(
        actions_for("cmd-b", &restarted),
        vec![OtherAction::name_for_type()]
    );
    assert!(actions_for("cmd-shift-k", &restarted).is_empty());

    let saved: BTreeMap<String, Option<String>> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(saved.is_empty());
}

#[test]
fn startup_rejects_saved_conflicts_and_allows_repair() {
    for (first_keys, other_keys) in [
        ("cmd-k", "super-k"),
        ("cmd-k", "cmd-k cmd-c"),
        ("cmd-k cmd-c", "cmd-k"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keybindings.json");
        let saved = serde_json::json!({
            FirstAction::name_for_type(): first_keys,
            OtherAction::name_for_type(): other_keys,
        })
        .to_string();
        std::fs::write(&path, &saved).unwrap();

        let mut app = TestApp::new();
        app.update(|cx| {
            load_overrides(path.clone(), cx).unwrap();
            register_commands(cx);

            let command = commands(cx)
                .into_iter()
                .find(|command| command.id == OtherAction::name_for_type())
                .unwrap();
            assert!(command.binding.is_none());
            assert!(
                command
                    .binding_error
                    .as_ref()
                    .unwrap()
                    .contains("First command")
            );
            assert!(command.is_modified());
        });

        assert_eq!(
            actions_for(first_keys, &app),
            vec![FirstAction::name_for_type()]
        );
        assert!(!actions_for(other_keys, &app).contains(&OtherAction::name_for_type()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);

        app.update(|cx| {
            // A failed edit must retain the startup error. Resetting to a free
            // default repairs the shortcut without requiring a restart.
            assert!(set_override(OtherAction::name_for_type(), Some(first_keys), cx).is_err());
            assert!(
                commands(cx)
                    .iter()
                    .any(|command| command.binding_error.is_some())
            );

            reset_command(OtherAction::name_for_type(), cx).unwrap();
            assert!(
                commands(cx)
                    .iter()
                    .all(|command| command.binding_error.is_none())
            );
        });

        let mut restarted = TestApp::new();
        restarted.update(|cx| {
            load_overrides(path.clone(), cx).unwrap();
            register_commands(cx);
            assert!(
                commands(cx)
                    .iter()
                    .all(|command| command.binding_error.is_none())
            );
        });

        assert_eq!(
            actions_for(first_keys, &restarted),
            vec![FirstAction::name_for_type()]
        );
        assert_eq!(
            actions_for("cmd-b", &restarted),
            vec![OtherAction::name_for_type()]
        );
    }
}

#[test]
fn startup_checks_defaults_against_saved_shortcuts_and_preserves_valid_swaps() {
    for swapped in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keybindings.json");
        let mut saved = serde_json::json!({FirstAction::name_for_type(): "cmd-b"});
        if swapped {
            saved[OtherAction::name_for_type()] = "cmd-q".into();
        }

        std::fs::write(&path, saved.to_string()).unwrap();

        let mut app = TestApp::new();
        app.update(|cx| {
            load_overrides(path, cx).unwrap();
            register_commands(cx);
            assert_eq!(
                commands(cx)
                    .iter()
                    .any(|command| command.binding_error.is_some()),
                !swapped
            );
        });

        assert_eq!(
            actions_for("cmd-b", &app),
            vec![FirstAction::name_for_type()]
        );
        assert_eq!(
            actions_for("cmd-q", &app),
            if swapped {
                vec![OtherAction::name_for_type()]
            } else {
                vec![]
            },
        );

        app.update(|cx| {
            reset_all(cx).unwrap();
            assert!(
                commands(cx)
                    .iter()
                    .all(|command| command.binding_error.is_none())
            );
        });

        assert_eq!(
            actions_for("cmd-q", &app),
            vec![FirstAction::name_for_type()]
        );
        assert_eq!(
            actions_for("cmd-b", &app),
            vec![OtherAction::name_for_type()]
        );
    }
}

#[test]
fn startup_keeps_fixed_shortcuts_and_can_reset_a_disabled_unassigned_command() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keybindings.json");
    std::fs::write(
        &path,
        serde_json::json!({FirstAction::name_for_type(): "cmd-q"}).to_string(),
    )
    .unwrap();

    let mut app = TestApp::new();
    app.update(|cx| {
        load_overrides(path.clone(), cx).unwrap();
        set_binding("cmd-q", OtherAction, None, cx).unwrap();
        register(
            FirstAction,
            "First command",
            "",
            "Workspace",
            None,
            None,
            cx,
        )
        .unwrap();

        let command = &commands(cx)[0];
        assert!(command.binding.is_none());
        assert!(command.binding_error.is_some());
        assert!(command.is_modified());

        reset_command(FirstAction::name_for_type(), cx).unwrap();
        assert!(commands(cx)[0].binding_error.is_none());
        assert!(!commands(cx)[0].is_modified());
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec![OtherAction::name_for_type()]
    );
    assert!(storage::load(&path).unwrap().is_empty());
}

#[test]
fn rejects_conflicts_including_aliases_and_chord_prefixes() {
    let mut app = TestApp::new();

    app.update(|cx| {
        register_commands(cx);

        assert!(matches!(
            set_override(FirstAction::name_for_type(), Some("super-b"), cx),
            Err(KeybindingError::Conflict("Other command"))
        ));
        assert!(set_override(FirstAction::name_for_type(), Some("cmd-b cmd-c"), cx).is_err());

        set_override(OtherAction::name_for_type(), Some("cmd-k cmd-c"), cx).unwrap();
        assert!(set_override(FirstAction::name_for_type(), Some("cmd-k"), cx).is_err());
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec![FirstAction::name_for_type()]
    );
}

#[test]
fn reset_all_restores_swapped_defaults_and_keeps_unmanaged_bindings() {
    let mut app = TestApp::new();

    app.update(|cx| {
        cx.bind_keys([KeyBinding::new("cmd-o", OtherAction, Some("Input"))]);
        register_commands(cx);

        set_override(FirstAction::name_for_type(), None, cx).unwrap();
        set_override(OtherAction::name_for_type(), Some("cmd-q"), cx).unwrap();
        set_override(FirstAction::name_for_type(), Some("cmd-b"), cx).unwrap();
        assert!(reset_command(FirstAction::name_for_type(), cx).is_err());

        reset_all(cx).unwrap();
        assert!(commands(cx).iter().all(|command| !command.is_modified()));

        let input = KeyContext::parse("Input").unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) =
            keymap.bindings_for_input(&[Keystroke::parse("cmd-o").unwrap()], &[input]);
        assert_eq!(bindings.len(), 1);
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec![FirstAction::name_for_type()]
    );
    assert_eq!(
        actions_for("cmd-b", &app),
        vec![OtherAction::name_for_type()]
    );
}

#[test]
fn failed_save_keeps_the_active_binding() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings").join("keybindings.json");
    let mut app = TestApp::new();

    app.update(|cx| {
        load_overrides(path, cx).unwrap();
        register_commands(cx);

        // A file where the settings directory should be forces a write failure.
        std::fs::write(directory.path().join("settings"), "occupied").unwrap();
        assert!(set_override(FirstAction::name_for_type(), Some("cmd-k"), cx).is_err());
        assert!(!commands(cx).iter().any(Command::is_modified));
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec![FirstAction::name_for_type()]
    );
    assert!(actions_for("cmd-k", &app).is_empty());
}

#[test]
fn corrupt_settings_are_not_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keybindings.json");
    std::fs::write(&path, "not json").unwrap();

    let mut app = TestApp::new();

    app.update(|cx| {
        assert!(load_overrides(path.clone(), cx).is_err());
        register_commands(cx);
        assert!(storage_error(cx).is_some());
        assert!(set_override(FirstAction::name_for_type(), Some("cmd-k"), cx).is_err());
    });

    assert_eq!(std::fs::read_to_string(path).unwrap(), "not json");
    assert_eq!(
        actions_for("cmd-q", &app),
        vec![FirstAction::name_for_type()]
    );
}

#[test]
fn reset_restores_an_unassigned_default() {
    let mut app = TestApp::new();

    app.update(|cx| {
        register(
            FirstAction,
            "First command",
            "",
            "Application",
            None,
            None,
            cx,
        )
        .unwrap();

        assert!(commands(cx)[0].binding.is_none());

        set_override(FirstAction::name_for_type(), Some("cmd-k"), cx).unwrap();
    });

    assert_eq!(
        actions_for("cmd-k", &app),
        vec![FirstAction::name_for_type()]
    );
    app.update(|cx| {
        reset_command(FirstAction::name_for_type(), cx).unwrap();
        assert!(commands(cx)[0].binding.is_none());
        assert!(!commands(cx)[0].is_modified());
    });

    assert!(actions_for("cmd-k", &app).is_empty());
}

#[test]
fn fixed_shortcuts_stay_out_of_settings_but_still_prevent_conflicts() {
    let mut app = TestApp::new();

    app.update(|cx| {
        set_binding("cmd-q", OtherAction, None, cx).unwrap();
        register(
            FirstAction,
            "First command",
            "",
            "Workspace",
            Some("cmd-b"),
            None,
            cx,
        )
        .unwrap();

        assert_eq!(commands(cx).len(), 1);
        assert!(matches!(
            set_override(FirstAction::name_for_type(), Some("cmd-q"), cx),
            Err(KeybindingError::Conflict(_))
        ));

        reset_all(cx).unwrap();
    });

    assert_eq!(
        actions_for("cmd-q", &app),
        vec![OtherAction::name_for_type()]
    );
}

fn actions_for(keystrokes: &str, app: &TestApp) -> Vec<&'static str> {
    app.read(|cx| {
        let keystrokes = keystrokes
            .split_whitespace()
            .map(Keystroke::parse)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) = keymap.bindings_for_input(&keystrokes, &[KeyContext::default()]);

        bindings
            .iter()
            .map(|binding| binding.action().name())
            .collect()
    })
}
