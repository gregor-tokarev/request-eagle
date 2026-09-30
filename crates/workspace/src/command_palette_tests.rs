use gpui_kit::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext, component::Root};

use crate::workspace::{Workspace, on_toggle_command_palette, on_toggle_sidebar};

/// A workspace with two requests in one collection, and the global
/// environments Production and Staging, kept until the directory is dropped.
fn workspace(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let catalog = environment::GlobalEnvironments::new(directory.path());
    for name in ["Production", "Staging"] {
        catalog.create(name).unwrap();
    }

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    let mut layout = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            Workspace::new(
                crate::performance::collections(2),
                catalog,
                updater::init("1.2.3", cx),
                window,
                cx,
            )
        });
        layout = Some(view.clone());

        Root::new(view, window, cx)
    });
    let layout = layout.unwrap();

    cx.update(|window, cx| {
        window.activate_window();
        on_toggle_sidebar(&layout, cx);
        on_toggle_command_palette(&layout, window.window_handle(), cx);
    });
    cx.run_until_parked();

    (layout, cx, directory)
}

fn palette_open(layout: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.run_until_parked();
    cx.read(|cx| {
        layout
            .read(cx)
            .command_palette
            .as_ref()
            .is_some_and(|palette| palette.upgrade().is_some())
    })
}

/// Open the palette and deliver the frame that shows its rows.
fn open_palette(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("secondary-k");
    next_frame(cx);
}

fn next_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
}

fn tab_titles(layout: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        layout
            .read(cx)
            .main_view
            .read(cx)
            .tabs
            .iter()
            .map(|tab| tab.title.to_string())
            .collect()
    })
}

#[gpui_kit::test]
fn shortcut_toggles_the_palette_and_restores_focus(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));

    open_palette(cx);
    assert!(palette_open(&layout, cx));
    assert!(cx.debug_bounds("command-palette").is_some());
    cx.update(|window, _| assert!(!sidebar_focus.is_focused(window)));

    cx.simulate_keystrokes("secondary-k");
    assert!(!palette_open(&layout, cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    // Escape closes the palette, even with a query.
    open_palette(cx);
    cx.simulate_input("tab");
    cx.simulate_keystrokes("escape");
    assert!(!palette_open(&layout, cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));
}

#[gpui_kit::test]
fn runs_commands_where_the_palette_was_opened(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);
    assert_eq!(tab_titles(&layout, cx).len(), 1);

    open_palette(cx);
    cx.simulate_input("new tab");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).len(), 2);

    // Commands that only apply elsewhere, such as closing settings, are not listed.
    open_palette(cx);
    cx.simulate_input("close settings");
    cx.simulate_keystrokes("enter");
    assert!(palette_open(&layout, cx));
    cx.read(|cx| assert!(!layout.read(cx).settings_visible));

    cx.simulate_keystrokes("escape");
    open_palette(cx);
    cx.simulate_input("toggle sidebar");
    cx.simulate_keystrokes("enter");
    assert!(!palette_open(&layout, cx));
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));
}

#[gpui_kit::test]
fn opens_requests_collections_and_environments(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);

    open_palette(cx);
    cx.simulate_input("resource 1");
    // Requests are searched in the background.
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).last().unwrap(), "Get resource 1");
    cx.update(|window, cx| {
        let layout = layout.read(cx);
        assert_eq!(layout.main_view.read(cx).selected, Some(1));
        assert!(window.focused(cx).is_some());
        assert!(!layout.sidebar.focus_handle(cx).is_focused(window));
    });

    open_palette(cx);
    cx.simulate_input("collection-00");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).last().unwrap(), "collection-00");
    cx.read(|cx| assert_eq!(layout.read(cx).main_view.read(cx).selected, Some(2)));

    open_palette(cx);
    cx.simulate_input("prod");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).last().unwrap(), "Production");
    cx.read(|cx| assert_eq!(layout.read(cx).main_view.read(cx).selected, Some(3)));
}

#[gpui_kit::test]
fn opens_over_settings_with_workspace_commands(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);

    cx.update(|window, cx| layout.update(cx, |layout, cx| layout.open_settings(window, cx)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("settings").is_some());

    // The workspace is drawn again before the palette reads its commands.
    cx.simulate_keystrokes("secondary-k");
    cx.read(|cx| assert!(!layout.read(cx).settings_visible));
    assert!(cx.debug_bounds("main-view").is_some());
    next_frame(cx);
    assert!(!palette_open(&layout, cx));
    next_frame(cx);
    assert!(palette_open(&layout, cx));
    next_frame(cx);

    cx.simulate_input("new tab");
    cx.simulate_keystrokes("enter");
    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).len(), 2);
}

#[gpui_kit::test]
fn runs_workspace_commands_after_hiding_the_focused_sidebar(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    // Focus stays on the sidebar, which is no longer rendered.
    cx.simulate_keystrokes("secondary-b");
    cx.run_until_parked();
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));

    open_palette(cx);
    cx.simulate_input("new tab");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).len(), 2);
}

#[gpui_kit::test]
fn clears_request_results_while_a_new_query_is_searched(cx: &mut TestAppContext) {
    let (layout, cx, _environments) = workspace(cx);

    open_palette(cx);
    cx.simulate_input("resource");
    cx.run_until_parked();

    let palette = cx.read(|cx| {
        layout
            .read(cx)
            .command_palette
            .as_ref()
            .and_then(|palette| palette.upgrade())
            .unwrap()
    });
    let request_count =
        |cx: &mut VisualTestContext| cx.read(|cx| palette.read(cx).delegate().request_count());
    assert_eq!(request_count(cx), 2);

    // Enter must not open a request for the previous query before the new
    // results arrive.
    cx.update(|window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("resource 1", window, cx)
        })
    });
    assert_eq!(request_count(cx), 0);

    cx.run_until_parked();
    assert_eq!(request_count(cx), 1);
}
