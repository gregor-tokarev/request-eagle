use environment::GlobalEnvironments;
use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};
use tab_ui::EnvironmentEditor;

use crate::main_view::Page;
use crate::workspace::{SidebarSection, Workspace};

fn workspace(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let catalog = GlobalEnvironments::new(directory.path());
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    let mut layout = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            Workspace::new(
                collection::CollectionRegistry::new(),
                catalog,
                updater::init("1.2.3", cx),
                window,
                cx,
            )
        });
        layout = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });

    (layout.unwrap(), cx, directory)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

fn active_editor(layout: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<EnvironmentEditor> {
    cx.read(|cx| {
        let view = layout.read(cx).main_view.read(cx);
        let Page::Environment(editor) = &view.tabs[view.selected.unwrap()].page else {
            panic!("an environment tab is selected");
        };

        editor.clone()
    })
}

#[gpui_kit::test]
fn creates_names_activates_and_deletes_a_global_environment(cx: &mut TestAppContext) {
    let (layout, cx, directory) = workspace(cx);
    cx.update(|window, cx| {
        layout.update(cx, |layout, cx| {
            layout.show_sidebar_section(SidebarSection::Environments, window, cx)
        })
    });

    // A new environment opens with its name selected, ready to be typed over.
    click(cx, "new-environment");
    cx.simulate_input("Staging");
    cx.simulate_keystrokes("enter");

    let editor = active_editor(&layout, cx);
    cx.read(|cx| {
        let view = layout.read(cx).main_view.read(cx);
        assert_eq!(view.tabs.len(), 2);
        assert_eq!(view.tabs[1].title, "Staging");
        assert_eq!(editor.read(cx).name, "Staging");
    });
    assert!(directory.path().join("Staging.toml").exists());

    // Variables are written when the workspace save shortcut runs.
    click(cx, "environment-key-0");
    cx.simulate_input("host");
    click(cx, "environment-value-0");
    cx.simulate_input("staging.example.com");
    cx.read(|cx| assert!(editor.read(cx).is_dirty()));
    cx.simulate_keystrokes("secondary-s");
    assert_eq!(
        environment::Environment::from_file(directory.path().join("Staging.toml"))
            .unwrap()
            .resolve("host"),
        Some("staging.example.com")
    );

    // The picker opens on the active choice and lists environments after
    // "No Environment".
    click(cx, "environment-picker");
    cx.simulate_keystrokes("down enter");
    let environments = cx.read(|cx| layout.read(cx).environment_panel.read(cx).environments());
    cx.read(|cx| assert_eq!(environments.read(cx).active().unwrap(), "Staging"));

    // Each opening starts with an empty search. A leftover "stag" would leave
    // only "Create new environment" to choose after typing "no env".
    for (search, active) in [
        ("no env", None),
        ("stag", Some("Staging")),
        ("no env", None),
    ] {
        click(cx, "environment-picker");
        cx.simulate_input(search);
        cx.simulate_keystrokes("enter");
        cx.read(|cx| {
            assert_eq!(
                environments.read(cx).active().map(|name| name.as_ref()),
                active
            );
            assert_eq!(environments.read(cx).names().len(), 1);
        });
    }

    cx.update(|_, cx| {
        environments.update(cx, |environments, cx| {
            environments.delete(&"Staging".into(), cx)
        })
    });
    cx.read(|cx| {
        assert_eq!(layout.read(cx).main_view.read(cx).tabs.len(), 1);
        assert!(environments.read(cx).active().is_none());
    });
    assert!(!directory.path().join("Staging.toml").exists());
}

#[gpui_kit::test]
fn creating_from_the_picker_closes_it_and_selects_the_new_name(cx: &mut TestAppContext) {
    let (layout, cx, directory) = workspace(cx);

    // The menu opens on "No Environment", then lists "Create new environment".
    click(cx, "environment-picker");
    cx.simulate_keystrokes("down enter");
    cx.simulate_input("QA");
    cx.simulate_keystrokes("enter");

    let editor = active_editor(&layout, cx);
    cx.read(|cx| assert_eq!(editor.read(cx).name, "QA"));
    assert!(directory.path().join("QA.toml").exists());
    let environments = cx.read(|cx| layout.read(cx).environment_panel.read(cx).environments());
    cx.read(|cx| assert!(environments.read(cx).active().is_none()));

    // The first click after creating reaches the table instead of a stale menu.
    click(cx, "environment-key-0");
    cx.simulate_input("host");
    cx.read(|cx| assert!(editor.read(cx).is_dirty()));
}
