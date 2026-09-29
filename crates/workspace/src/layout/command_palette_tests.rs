use gpui_kit::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext, component::Root};

use crate::workspace::{Layout, on_toggle_command_palette, on_toggle_sidebar};

fn workspace(cx: &mut TestAppContext) -> (Entity<Layout>, &mut VisualTestContext) {
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
            Layout::new(
                crate::performance::collections(2),
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

    (layout, cx)
}

fn palette_open(layout: &Entity<Layout>, cx: &mut VisualTestContext) -> bool {
    cx.run_until_parked();
    cx.read(|cx| {
        layout
            .read(cx)
            .command_palette
            .as_ref()
            .is_some_and(|palette| palette.upgrade().is_some())
    })
}

fn tab_titles(layout: &Entity<Layout>, cx: &mut VisualTestContext) -> Vec<String> {
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
    let (layout, cx) = workspace(cx);
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));

    cx.simulate_keystrokes("secondary-k");
    assert!(palette_open(&layout, cx));
    assert!(cx.debug_bounds("command-palette").is_some());
    cx.update(|window, _| assert!(!sidebar_focus.is_focused(window)));

    cx.simulate_keystrokes("secondary-k");
    assert!(!palette_open(&layout, cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    // Escape clears the query first, then closes the palette.
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("tab");
    cx.simulate_keystrokes("escape");
    assert!(palette_open(&layout, cx));
    cx.simulate_keystrokes("escape");
    assert!(!palette_open(&layout, cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));
}

#[gpui_kit::test]
fn runs_commands_where_the_palette_was_opened(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    assert_eq!(tab_titles(&layout, cx).len(), 1);

    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("new tab");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).len(), 2);

    // Commands that only apply elsewhere, such as closing settings, are not listed.
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("close settings");
    cx.simulate_keystrokes("enter");
    assert!(palette_open(&layout, cx));
    cx.read(|cx| assert!(!layout.read(cx).settings_visible));

    cx.simulate_keystrokes("escape escape");
    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("toggle sidebar");
    cx.simulate_keystrokes("enter");
    assert!(!palette_open(&layout, cx));
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));
}

#[gpui_kit::test]
fn opens_requests_and_reveals_collections(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);

    cx.simulate_keystrokes("secondary-k");
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

    cx.simulate_keystrokes("secondary-b");
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));

    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("collection-00");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    cx.update(|window, cx| {
        let layout = layout.read(cx);
        assert!(*layout.sidebar_visible.read(cx));
        assert!(layout.sidebar.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn shortcut_returns_from_settings_to_the_workspace(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);

    cx.update(|window, cx| layout.update(cx, |layout, cx| layout.open_settings(window, cx)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("settings").is_some());

    // The palette itself opens on the next platform frame, once the
    // workspace commands are rendered; test windows do not deliver frames.
    cx.simulate_keystrokes("secondary-k");
    cx.run_until_parked();
    cx.read(|cx| assert!(!layout.read(cx).settings_visible));
    assert!(cx.debug_bounds("main-view").is_some());
}

#[gpui_kit::test]
fn runs_workspace_commands_after_hiding_the_focused_sidebar(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    // Focus stays on the sidebar, which is no longer rendered.
    cx.simulate_keystrokes("secondary-b");
    cx.run_until_parked();
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));

    cx.simulate_keystrokes("secondary-k");
    cx.simulate_input("new tab");
    cx.simulate_keystrokes("enter");

    assert!(!palette_open(&layout, cx));
    assert_eq!(tab_titles(&layout, cx).len(), 2);
}
