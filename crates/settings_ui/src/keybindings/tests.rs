use super::{KeybindingsPage, matches_search};
use crate::CloseSettings;
use gpui_kit::{
    Action, AppContext as _, Context, Entity, Global, InteractiveElement as _, IntoElement,
    Keystroke, Modifiers, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, actions, div, px,
};
use keybindings_service as keybindings;

actions!(settings_tests, [Quit, OpenSettings, ToggleLeftSidebar]);

#[derive(Default)]
struct QuitCount(usize);

impl Global for QuitCount {}

// Host the search bar and command rows to exercise layout, clicks, and key dispatch.
struct RecorderHarness {
    page: Entity<KeybindingsPage>,
    outside: gpui_kit::FocusHandle,
    width: gpui_kit::Pixels,
}

impl Render for RecorderHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page.update(cx, |page, cx| {
            let mut commands = keybindings::commands(cx);
            commands.sort_by_key(|command| command.label);
            commands.retain(|command| page.matches_search(command, ""));

            gpui_kit::component::v_flex()
                .w(self.width)
                .child(
                    div()
                        .id("outside-recorder")
                        .debug_selector(|| "outside-recorder".into())
                        .w_full()
                        .h_4()
                        .track_focus(&self.outside),
                )
                .child(page.render_search(window, cx))
                .children(
                    commands
                        .iter()
                        .map(|command| page.render_command(command, self.width < px(680.), cx)),
                )
        })
    }
}

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<KeybindingsPage>,
    gpui_kit::FocusHandle,
    &mut VisualTestContext,
) {
    setup_width(cx, px(880.))
}

fn setup_width(
    cx: &mut TestAppContext,
    width: gpui_kit::Pixels,
) -> (
    Entity<KeybindingsPage>,
    gpui_kit::FocusHandle,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init_commands(cx);

        keybindings::register(
            Quit,
            "Quit",
            "Quit the application",
            "Application",
            Some("cmd-q"),
            None,
            cx,
        )
        .unwrap();

        cx.set_global(QuitCount::default());
        cx.on_action(|_: &Quit, cx| cx.global_mut::<QuitCount>().0 += 1);
    });

    let (host, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| KeybindingsPage::new(window, cx));
        cx.observe(&view, |_, _, cx| cx.notify()).detach();

        RecorderHarness {
            page: view,
            outside: cx.focus_handle(),
            width,
        }
    });

    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();

    let (page, outside) = host.read_with(cx, |host, _| (host.page.clone(), host.outside.clone()));

    (page, outside, cx)
}

fn start(page: &Entity<KeybindingsPage>, id: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let command = keybindings::commands(cx)
            .into_iter()
            .find(|c| c.id == id)
            .unwrap();

        page.update(cx, |page, cx| page.start_recording(command, window, cx));
    });
}

fn click_recorder_button(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(selector)
        .expect("recorder button should be rendered");
    cx.simulate_click(bounds.center(), Modifiers::default());
}

#[gpui_kit::test]
fn recording_intercepts_commands_and_saves_a_replacement(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    cx.update(|window, cx| {
        let command = keybindings::commands(cx)
            .into_iter()
            .find(|command| command.id == Quit::name_for_type())
            .unwrap();

        page.update(cx, |page, cx| page.start_recording(command, window, cx));

        // A fast first key must be intercepted before the recorder's first paint.
        window.dispatch_keystroke(Keystroke::parse("cmd-q").unwrap(), cx);
    });
    cx.read(|cx| {
        assert_eq!(cx.global::<QuitCount>().0, 0);
        assert_eq!(
            page.read(cx)
                .recording
                .as_ref()
                .unwrap()
                .keystroke
                .as_ref()
                .unwrap()
                .unparse(),
            "cmd-q"
        );
    });
    cx.simulate_keystrokes("cmd-shift-q");
    click_recorder_button("save-recording", cx);

    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert_eq!(
            keybindings::binding_for::<Quit>(cx).unwrap().keystrokes,
            Keystroke::parse("cmd-shift-q").unwrap().unparse()
        );
    });
    cx.simulate_keystrokes("cmd-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 0));
    cx.simulate_keystrokes("cmd-shift-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 1));
}

#[gpui_kit::test]
fn conflict_and_cancel_preserve_existing_shortcuts(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    start(&page, ToggleLeftSidebar::name_for_type(), cx);
    cx.simulate_keystrokes("cmd-,");
    click_recorder_button("save-recording", cx);

    cx.read(|cx| {
        assert!(
            page.read(cx)
                .recording
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("Open settings")
        );
    });
    cx.simulate_keystrokes("cmd-shift-b");

    cx.read(|cx| assert!(page.read(cx).recording.as_ref().unwrap().error.is_none()));
    click_recorder_button("cancel-recording", cx);

    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert_eq!(
            keybindings::binding_for::<ToggleLeftSidebar>(cx)
                .unwrap()
                .keystrokes,
            "cmd-b"
        );
        assert_eq!(
            keybindings::binding_for::<OpenSettings>(cx)
                .unwrap()
                .keystrokes,
            "cmd-,"
        );
    });
}

#[gpui_kit::test]
fn inline_recorder_releases_capture_when_focus_leaves(cx: &mut TestAppContext) {
    let (page, outside, cx) = setup(cx);

    start(&page, ToggleLeftSidebar::name_for_type(), cx);
    cx.simulate_keystrokes("cmd-shift-b");
    cx.update(|window, cx| {
        window.focus(&outside, cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert_eq!(
            keybindings::binding_for::<ToggleLeftSidebar>(cx)
                .unwrap()
                .keystrokes,
            "cmd-b"
        );
    });
    cx.simulate_keystrokes("cmd-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 1));
}

#[gpui_kit::test]
fn special_keys_are_recorded_and_never_dispatch_while_capturing(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    // Free Escape so it can be assigned to our harmless Quit action.
    cx.update(|_, cx| {
        keybindings::set_override(CloseSettings::name_for_type(), None, cx).unwrap();
    });

    let mut dispatched = 0;
    for key in [
        "escape",
        "enter",
        "tab",
        "shift-tab",
        "backspace",
        "delete",
        "x",
    ] {
        let before = cx.read(|cx| {
            keybindings::binding_for::<Quit>(cx)
                .unwrap()
                .keystrokes
                .clone()
        });

        start(&page, Quit::name_for_type(), cx);
        cx.simulate_keystrokes(key);

        cx.read(|cx| {
            let recording = page
                .read(cx)
                .recording
                .as_ref()
                .expect("Key must not end recording");
            assert_eq!(recording.keystroke.as_ref().unwrap().unparse(), key);
            assert!(recording.error.is_none(), "{key}: {:?}", recording.error);
            assert_eq!(
                keybindings::binding_for::<Quit>(cx).unwrap().keystrokes,
                before
            );
            assert_eq!(cx.global::<QuitCount>().0, dispatched);
        });
        click_recorder_button("save-recording", cx);

        cx.read(|cx| assert!(page.read(cx).recording.is_none()));
        cx.simulate_keystrokes(key);
        dispatched += 1;
        cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, dispatched));

        // The same key now has an application action. It must still be captured,
        // including Tab, which must not navigate out and release the recorder.
        start(&page, Quit::name_for_type(), cx);
        cx.simulate_keystrokes(key);

        cx.read(|cx| {
            assert!(page.read(cx).recording.is_some());
            assert_eq!(cx.global::<QuitCount>().0, dispatched);
        });
        click_recorder_button("cancel-recording", cx);
        cx.simulate_keystrokes(key);
        dispatched += 1;
        cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, dispatched));
    }
}

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

#[gpui_kit::test]
fn recorder_buttons_save_cancel_and_remove(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    start(&page, Quit::name_for_type(), cx);
    cx.simulate_keystrokes("cmd-shift-q");
    click_recorder_button("cancel-recording", cx);

    cx.read(|cx| assert!(page.read(cx).recording.is_none()));
    cx.simulate_keystrokes("cmd-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 1));

    start(&page, Quit::name_for_type(), cx);
    cx.simulate_keystrokes("cmd-shift-q");
    click_recorder_button("save-recording", cx);

    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert_eq!(
            keybindings::binding_for::<Quit>(cx).unwrap().keystrokes,
            "cmd-shift-q"
        );
    });
    cx.simulate_keystrokes("cmd-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 1));
    cx.simulate_keystrokes("cmd-shift-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 2));

    click_recorder_button("remove-settings_tests::Quit", cx);

    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert!(keybindings::binding_for::<Quit>(cx).is_none());
    });
    cx.simulate_keystrokes("cmd-shift-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 2));

    click_recorder_button("reset-settings_tests::Quit", cx);
    start(&page, Quit::name_for_type(), cx);
    click_recorder_button("remove-recording", cx);

    cx.read(|cx| {
        assert!(page.read(cx).recording.is_none());
        assert!(keybindings::binding_for::<Quit>(cx).is_none());
    });
    cx.simulate_keystrokes("cmd-q");

    cx.read(|cx| assert_eq!(cx.global::<QuitCount>().0, 2));
}

#[gpui_kit::test]
fn recording_keeps_command_rows_and_shortcut_column_in_place(cx: &mut TestAppContext) {
    for width in [px(320.), px(880.)] {
        let (page, _, cx) = setup_width(cx, width);

        let selectors = [
            "keybinding-row-settings_tests::OpenSettings",
            "keybinding-row-settings_tests::ToggleLeftSidebar",
            "shortcut-slot-settings_tests::ToggleLeftSidebar",
        ];
        let before = selectors.map(|selector| cx.debug_bounds(selector).unwrap());
        let record = cx
            .debug_bounds("record-settings_tests::ToggleLeftSidebar")
            .unwrap();
        cx.simulate_click(record.center(), Modifiers::default());

        cx.read(|cx| assert!(page.read(cx).recording.is_some()));
        for keys in [
            "",
            "cmd-ctrl-alt-shift-b",
            "cmd-,",
            "escape",
            "tab",
            "enter",
        ] {
            if !keys.is_empty() {
                cx.simulate_keystrokes(keys);
            }

            for (selector, expected) in selectors.iter().zip(before) {
                assert_eq!(
                    cx.debug_bounds(selector).unwrap(),
                    expected,
                    "{selector}, {width:?}, {keys}"
                );
            }
        }

        click_recorder_button("cancel-recording", cx);

        cx.read(|cx| assert!(page.read(cx).recording.is_none()));
        for (selector, expected) in selectors.iter().zip(before) {
            assert_eq!(cx.debug_bounds(selector).unwrap(), expected);
        }
    }
}

#[gpui_kit::test]
fn shortcut_search_captures_filters_clears_and_releases_focus(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.search_by_shortcut = true;
            page.focus_search(window, cx);
            cx.notify();
        });

        // Capture immediately, even before the search recorder has been painted.
        window.dispatch_keystroke(Keystroke::parse("cmd-q").unwrap(), cx);
    });
    cx.run_until_parked();

    cx.read(|cx| {
        assert_eq!(cx.global::<QuitCount>().0, 0);
        assert_eq!(
            page.read(cx).search_keystroke.as_ref().unwrap().unparse(),
            "cmd-q"
        );
        assert_eq!(
            keybindings::binding_for::<Quit>(cx).unwrap().keystrokes,
            "cmd-q"
        );
    });
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::Quit")
            .is_some()
    );
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::ToggleLeftSidebar")
            .is_none()
    );

    cx.simulate_keystrokes("cmd-shift-q");
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::Quit")
            .is_none()
    );

    for key in [
        "escape",
        "enter",
        "tab",
        "shift-tab",
        "backspace",
        "delete",
        "x",
    ] {
        cx.simulate_keystrokes(key);
        cx.read(|cx| {
            assert_eq!(
                page.read(cx).search_keystroke.as_ref().unwrap().unparse(),
                key
            )
        });
    }

    click_recorder_button("clear-shortcut-search", cx);
    cx.read(|cx| {
        let page = page.read(cx);
        assert!(page.search_keystroke.is_none());
        assert!(!page.search_by_shortcut);
        assert!(page.search.read(cx).value().is_empty());
    });
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::Quit")
            .is_some()
    );
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::ToggleLeftSidebar")
            .is_some()
    );

    click_recorder_button("toggle-shortcut-search", cx);
    cx.simulate_keystrokes("cmd-b");
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::Quit")
            .is_none()
    );
    assert!(
        cx.debug_bounds("keybinding-row-settings_tests::ToggleLeftSidebar")
            .is_some()
    );

    let outside = cx.debug_bounds("outside-recorder").unwrap();
    cx.simulate_click(outside.center(), Modifiers::default());
    cx.simulate_keystrokes("cmd-q");
    cx.read(|cx| {
        assert_eq!(cx.global::<QuitCount>().0, 1);
        assert_eq!(
            page.read(cx).search_keystroke.as_ref().unwrap().unparse(),
            "cmd-b"
        );
    });

    click_recorder_button("toggle-shortcut-search", cx);
    cx.read(|cx| {
        assert!(!page.read(cx).search_by_shortcut);
        assert!(page.read(cx).search_keystroke.is_none());
    });
}

#[gpui_kit::test]
fn shortcut_search_matches_keys_and_modifiers_without_using_command_text(cx: &mut TestAppContext) {
    let (page, _, cx) = setup(cx);

    cx.update(|_, cx| {
        page.update(cx, |page, cx| {
            page.search_by_shortcut = true;
            let command = keybindings::commands(cx)
                .into_iter()
                .find(|command| command.id == ToggleLeftSidebar::name_for_type())
                .unwrap();

            for (keys, expected) in [
                ("cmd-b", true),
                ("b", false),
                ("cmd-shift-b", false),
                ("ctrl-b", false),
                ("cmd-q", false),
            ] {
                let mut stroke = Keystroke::parse(keys).unwrap();
                stroke.key_char = Some("b".into());
                page.record_search_key(&stroke, cx);
                assert_eq!(page.matches_search(&command, "sidebar"), expected, "{keys}");
            }

            let before = page.search_keystroke.clone();
            for key in ["", "cmd", "shift", "ctrl", "alt", "fn"] {
                page.record_search_key(
                    &Keystroke {
                        key: key.into(),
                        ..Default::default()
                    },
                    cx,
                );
                assert_eq!(page.search_keystroke, before);
            }

            let mut unassigned = command.clone();
            unassigned.binding = None;
            assert!(!page.matches_search(&unassigned, ""));
        });
    });
}

#[gpui_kit::test]
fn search_mode_switch_keeps_the_search_bar_and_rows_in_place(cx: &mut TestAppContext) {
    for width in [px(320.), px(880.)] {
        let (page, _, cx) = setup_width(cx, width);
        let selectors = [
            "keybindings-search",
            "toggle-shortcut-search",
            "keybinding-row-settings_tests::ToggleLeftSidebar",
        ];
        let before = selectors.map(|selector| cx.debug_bounds(selector).unwrap());

        for _ in 0..2 {
            click_recorder_button("toggle-shortcut-search", cx);
            cx.read(|cx| assert!(page.read(cx).search_by_shortcut));

            for (selector, expected) in selectors.iter().zip(before) {
                assert_eq!(
                    cx.debug_bounds(selector).unwrap(),
                    expected,
                    "{selector}, {width:?}"
                );
            }

            // The cross exits recorder mode even before a shortcut is recorded.
            click_recorder_button("clear-shortcut-search", cx);
            cx.read(|cx| assert!(!page.read(cx).search_by_shortcut));

            for (selector, expected) in selectors.iter().zip(before) {
                assert_eq!(
                    cx.debug_bounds(selector).unwrap(),
                    expected,
                    "{selector}, {width:?}"
                );
            }
        }

        cx.simulate_input("sidebar");
        cx.read(|cx| assert_eq!(page.read(cx).search.read(cx).value(), "sidebar"));
        click_recorder_button("toggle-shortcut-search", cx);
        cx.simulate_keystrokes("cmd-b");
        click_recorder_button("clear-shortcut-search", cx);
        cx.read(|cx| {
            let page = page.read(cx);
            assert!(!page.search_by_shortcut);
            assert!(page.search_keystroke.is_none());
            assert!(page.search.read(cx).value().is_empty());
        });

        cx.simulate_input("settings");
        cx.read(|cx| assert_eq!(page.read(cx).search.read(cx).value(), "settings"));
    }
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
