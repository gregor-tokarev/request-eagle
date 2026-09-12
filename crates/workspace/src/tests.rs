use crate::actions::{CloseSettings, ToggleLeftSidebar};
use crate::layout::bottom_panel::TOGGLE_SIDEBAR_BUTTON;
use crate::workspace::{Layout, on_toggle_sidebar};
use collection::CollectionRegistry;
use gpui_kit::{Focusable, Modifiers, TestAppContext, px};
use std::sync::Arc;

#[gpui_kit::test]
fn settings_survives_closing_and_reopening(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    let (layout, cx) = cx.add_window_view(|window, cx| {
        Layout::new(
            Arc::new(CollectionRegistry::new()),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    });

    let settings = cx.read(|cx| layout.read(cx).settings.clone());
    assert!(cx.debug_bounds("settings").is_none());
    assert!(cx.debug_bounds("main-view").is_some());
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    for _ in 0..2 {
        cx.update(|window, cx| {
            layout.update(cx, |layout, cx| layout.open_settings(window, cx));
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("settings").is_some());
        assert!(cx.debug_bounds("main-view").is_none());
        cx.read(|cx| {
            assert!(layout.read(cx).settings_visible);
            assert_eq!(layout.read(cx).settings, settings);
        });

        // Reopening an already visible screen must not replace the saved focus.
        cx.update(|window, cx| {
            layout.update(cx, |layout, cx| layout.open_settings(window, cx));
            window.dispatch_action(Box::new(CloseSettings), cx);
        });
        cx.run_until_parked();

        cx.read(|cx| {
            assert!(!layout.read(cx).settings_visible);
            assert_eq!(layout.read(cx).settings, settings);
        });
        assert!(cx.debug_bounds("settings").is_none());
        assert!(cx.debug_bounds("main-view").is_some());
        cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));
    }
}

#[gpui_kit::test]
fn toggle_sidebar_action(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
        crate::actions::init(cx);
    });

    let (layout, cx) = cx.add_window_view(|window, cx| {
        Layout::new(
            Arc::new(CollectionRegistry::new()),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    });
    cx.update(|_, cx| on_toggle_sidebar(&layout, cx));

    let sidebar_visible =
        |cx: &TestAppContext| cx.read(|cx| *layout.read(cx).sidebar_visible.read(cx));

    assert!(sidebar_visible(cx));

    let main_split = cx.read(|cx| layout.read(cx).main_split.clone());
    cx.update(|window, cx| {
        main_split.update(cx, |split, cx| split.resize_panel(0, px(300.), window, cx));
    });
    cx.run_until_parked();

    let panel_sizes = cx.read(|cx| main_split.read(cx).sizes().clone());
    assert_eq!(panel_sizes[0], px(300.));

    let main_bounds = cx
        .debug_bounds("main-view")
        .expect("main view should be rendered");

    // The tooltip hint shows this binding.
    cx.update(|window, _| {
        let binding = window
            .highest_precedence_binding_for_action(&ToggleLeftSidebar)
            .expect("ToggleLeftSidebar should be bound");
        assert_eq!(binding.keystrokes()[0].inner().to_string(), "⌘B");
    });

    cx.simulate_keystrokes("cmd-b");
    assert!(!sidebar_visible(cx));

    let expanded_bounds = cx
        .debug_bounds("main-view")
        .expect("main view should remain rendered");
    assert_eq!(
        expanded_bounds.size.width,
        main_bounds.size.width + px(300.)
    );

    cx.simulate_keystrokes("cmd-b");
    assert!(sidebar_visible(cx));

    assert_eq!(cx.debug_bounds("main-view"), Some(main_bounds));

    assert_eq!(
        cx.read(|cx| main_split.read(cx).sizes().clone()),
        panel_sizes
    );

    let button_bounds = cx
        .debug_bounds(TOGGLE_SIDEBAR_BUTTON)
        .expect("toggle-sidebar button should be rendered");
    cx.simulate_click(button_bounds.center(), Modifiers::default());
    assert!(!sidebar_visible(cx));
}
