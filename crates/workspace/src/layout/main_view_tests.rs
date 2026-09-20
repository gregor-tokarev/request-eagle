use std::path::Path;

use gpui_kit::{
    AppContext, Context, Entity, InputEvent as _, InteractiveElement, IntoElement, Modifiers,
    Render, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, Window,
    div, point, px, size,
};

use super::main_view::MainView;
use crate::workspace::Layout;

fn workspace(cx: &mut TestAppContext) -> (Entity<Layout>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    cx.add_window_view(|window, cx| {
        Layout::new(
            collection::CollectionRegistry::new(),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    })
}

#[gpui_kit::test]
fn tab_shortcuts_work_from_sidebar_and_wrap(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    // Initial focus is in the collections sidebar, outside the main view.
    cx.simulate_keystrokes("secondary-t secondary-t");
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(2));
    });

    for (keys, selected) in [
        ("secondary-}", 0),
        ("secondary-{", 2),
        ("secondary-{", 1),
        ("secondary-1", 0),
        ("secondary-3", 2),
        ("secondary-2", 1),
        ("secondary-8", 1),
        ("secondary-9", 2),
    ] {
        cx.simulate_keystrokes(keys);
        cx.read(|cx| assert_eq!(view.read(cx).selected, Some(selected), "{keys}"));
    }

    cx.simulate_keystrokes("secondary-w secondary-w secondary-w secondary-w");
    cx.simulate_keystrokes("secondary-{");
    cx.simulate_keystrokes("secondary-}");
    cx.simulate_keystrokes("secondary-9");
    cx.read(|cx| {
        assert!(view.read(cx).tabs.is_empty());
        assert_eq!(view.read(cx).selected, None);
    });
    assert!(cx.debug_bounds("main-view").is_some());

    cx.simulate_keystrokes("secondary-t");
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 1);
        assert_eq!(view.read(cx).tabs[0].id, 4);
        assert_eq!(view.read(cx).selected, Some(0));
    });
}

#[gpui_kit::test]
fn tab_buttons_preserve_selection_when_closing_other_tabs(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    for _ in 0..2 {
        let bounds = cx.debug_bounds("new-tab").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    let first_tab = cx.debug_bounds("page-tab-1").unwrap();
    cx.simulate_click(first_tab.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(view.read(cx).selected, Some(0)));

    let second_tab = cx.debug_bounds("page-tab-2").unwrap();
    cx.simulate_mouse_move(second_tab.center(), None, Modifiers::default());
    let close_second = cx.debug_bounds("close-tab-2").unwrap();
    cx.simulate_mouse_move(close_second.center(), None, Modifiers::default());
    cx.simulate_click(close_second.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 2);
        assert_eq!(view.read(cx).selected, Some(0));
        assert_eq!(view.read(cx).tabs[0].id, 1);
    });

    cx.simulate_keystrokes("secondary-2");
    cx.simulate_mouse_move(first_tab.center(), None, Modifiers::default());
    let close_first = cx.debug_bounds("close-tab-1").unwrap();
    cx.simulate_mouse_move(close_first.center(), None, Modifiers::default());
    cx.simulate_click(close_first.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).selected, Some(0));
        assert_eq!(view.read(cx).tabs[0].id, 3);
    });

    let last_tab = cx.debug_bounds("page-tab-3").unwrap();
    cx.simulate_mouse_move(last_tab.center(), None, Modifiers::default());
    let close_last = cx.debug_bounds("close-tab-3").unwrap();
    cx.simulate_mouse_move(close_last.center(), None, Modifiers::default());
    cx.simulate_click(close_last.center(), Modifiers::default());
    assert!(cx.debug_bounds("page-tab-3").is_none());

    let new_tab = cx.debug_bounds("new-tab").unwrap();
    cx.simulate_click(new_tab.center(), Modifiers::default());
    assert!(cx.debug_bounds("page-tab-4").is_some());
}

#[gpui_kit::test]
fn overflowing_tabs_scroll_to_selected_position(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    for _ in 0..12 {
        cx.simulate_keystrokes("secondary-t");
    }

    let tab_bar = cx.debug_bounds("main-tab-bar").unwrap();
    let last_tab = cx.debug_bounds("page-tab-13").unwrap();
    let new_tab = cx.debug_bounds("new-tab").unwrap();
    assert!(last_tab.left() >= tab_bar.left());
    assert!(last_tab.right() <= new_tab.left());
    assert!(new_tab.right() <= tab_bar.right());

    for (shortcut, index, selector) in [
        ("secondary-1", 0, "page-tab-1"),
        ("secondary-2", 1, "page-tab-2"),
        ("secondary-3", 2, "page-tab-3"),
        ("secondary-4", 3, "page-tab-4"),
        ("secondary-5", 4, "page-tab-5"),
        ("secondary-6", 5, "page-tab-6"),
        ("secondary-7", 6, "page-tab-7"),
        ("secondary-8", 7, "page-tab-8"),
        ("secondary-9", 12, "page-tab-13"),
    ] {
        cx.simulate_keystrokes(shortcut);
        cx.read(|cx| assert_eq!(view.read(cx).selected, Some(index)));

        let selected_tab = cx.debug_bounds(selector).unwrap();
        assert!(selected_tab.left() >= tab_bar.left());
        assert!(selected_tab.right() <= new_tab.left());
    }
}

#[gpui_kit::test]
fn thousands_of_tabs_keep_selection_visible_and_offscreen_tabs_unrendered(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for _ in 1..10_000 {
                view.new_tab(cx);
            }
        });
    });

    for (width, height) in [(1024., 768.), (3440., 1410.), (1440., 900.)] {
        cx.simulate_resize(size(px(width), px(height)));

        for (shortcut, selected, visible, hidden) in [
            ("secondary-9", 9_999, "page-tab-10000", "page-tab-1"),
            ("secondary-}", 0, "page-tab-1", "page-tab-10000"),
            ("secondary-{", 9_999, "page-tab-10000", "page-tab-1"),
            ("secondary-{", 9_998, "page-tab-9999", "page-tab-1"),
            ("secondary-1", 0, "page-tab-1", "page-tab-9999"),
        ] {
            cx.simulate_keystrokes(shortcut);
            cx.read(|cx| assert_eq!(view.read(cx).selected, Some(selected)));

            let tab = cx.debug_bounds(visible).unwrap();
            let bar = cx.debug_bounds("main-tab-bar").unwrap();
            let new_tab = cx.debug_bounds("new-tab").unwrap();
            assert!(tab.left() >= bar.left(), "{visible} at {width}");
            assert!(tab.right() <= new_tab.left(), "{visible} at {width}");
            assert!(cx.debug_bounds(hidden).is_none());
        }
    }
}

#[gpui_kit::test]
fn virtual_tabs_support_mouse_scrolling_selection_and_close(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for _ in 1..1_000 {
                view.new_tab(cx);
            }
        });
    });
    cx.simulate_keystrokes("secondary-9");

    let last_tab = cx.debug_bounds("page-tab-1000").unwrap();
    cx.update(|window, cx| {
        window.dispatch_event(
            ScrollWheelEvent {
                position: last_tab.center(),
                delta: ScrollDelta::Pixels(point(px(1800.), px(0.))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    });
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(view.read(cx).selected, Some(999)));
    assert!(cx.debug_bounds("page-tab-1000").is_none());

    let tab = cx.debug_bounds("page-tab-990").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(view.read(cx).selected, Some(989)));

    cx.simulate_mouse_move(tab.center(), None, Modifiers::default());
    let close = cx.debug_bounds("close-tab-990").unwrap();
    cx.simulate_mouse_move(close.center(), None, Modifiers::default());
    cx.simulate_click(close.center(), Modifiers::default());
    cx.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.tabs.len(), 999);
        assert_eq!(view.tabs[view.selected.unwrap()].id, 991);
    });
    assert!(cx.debug_bounds("page-tab-990").is_none());
    assert!(cx.debug_bounds("page-tab-991").is_some());

    cx.simulate_keystrokes("secondary-9 secondary-w");
    assert!(cx.debug_bounds("page-tab-999").is_some());
    assert!(cx.debug_bounds("page-tab-1000").is_none());
}

#[gpui_kit::test]
fn settings_do_not_change_hidden_tabs(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    cx.simulate_keystrokes("secondary-t");
    cx.update(|window, cx| {
        layout.update(cx, |layout, cx| layout.open_settings(window, cx));
    });
    cx.simulate_keystrokes("secondary-t secondary-w secondary-1");
    cx.simulate_keystrokes("secondary-{");
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 2);
        assert_eq!(view.read(cx).selected, Some(1));
    });

    cx.simulate_keystrokes("escape secondary-w");
    cx.read(|cx| assert_eq!(view.read(cx).tabs.len(), 1));
}

#[gpui_kit::test]
fn sidebar_requests_open_reuse_and_reopen_tabs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });

    let (layout, cx) = cx.add_window_view(|window, cx| {
        Layout::new(
            crate::performance::collections(2),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    });
    let view = cx.read(|cx| layout.read(cx).main_view.clone());

    // Folders still collapse and expand without opening tabs.
    let folder = cx.debug_bounds("collection-row-1").unwrap();
    cx.simulate_click(folder.center(), Modifiers::default());
    assert!(cx.debug_bounds("collection-row-2").is_none());
    cx.simulate_click(folder.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(view.read(cx).tabs.len(), 1));

    let first_request = cx.debug_bounds("collection-row-2").unwrap();
    cx.simulate_click(first_request.center(), Modifiers::default());
    let first_page = cx.read(|cx| {
        let view = view.read(cx);

        assert_eq!(view.tabs.len(), 2);
        assert_eq!(view.selected, Some(1));
        assert_eq!(view.tabs[1].title, "Get resource 0");
        assert_eq!(view.tabs[1].method, Some("GET"));

        view.tabs[1].page.entity_id()
    });

    let second_request = cx.debug_bounds("collection-row-3").unwrap();
    assert!(cx.debug_bounds("tab-method-2").is_some());
    cx.simulate_click(second_request.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(2));
        assert_eq!(view.read(cx).tabs[2].title, "Get resource 1");
    });

    cx.simulate_click(first_request.center(), Modifiers::default());
    cx.simulate_click(first_request.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(1));
        assert_eq!(view.read(cx).tabs[1].page.entity_id(), first_page);
    });

    cx.simulate_keystrokes("secondary-w");
    cx.simulate_click(first_request.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(2));
        assert_eq!(view.read(cx).tabs[2].title, "Get resource 0");
        assert_ne!(view.read(cx).tabs[2].page.entity_id(), first_page);
    });
}

#[gpui_kit::test]
fn request_tabs_use_file_identity_and_refresh_names_when_reopened(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_, cx| MainView::new(cx));
    let first_path = Path::new("/collection/first.toml");
    let second_path = Path::new("/collection/second.toml");

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_request(first_path, "Same name".into(), "GET", cx);
            view.open_request(second_path, "Same name".into(), "POST", cx);
        });
    });
    let first_page = cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);

        view.read(cx).tabs[1].page.entity_id()
    });

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_request(first_path, "Renamed request".into(), "PUT", cx);
        });
    });
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(1));
        assert_eq!(view.read(cx).tabs[1].title, "Renamed request");
        assert_eq!(view.read(cx).tabs[1].method, Some("PUT"));
        assert_eq!(view.read(cx).tabs[1].page.entity_id(), first_page);
        assert_eq!(view.read(cx).tabs[2].title, "Same name");
        assert_eq!(view.read(cx).tabs[2].method, Some("POST"));
    });
}

struct StatefulPage(usize);

impl Render for StatefulPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let value = self.0;

        div().debug_selector(move || format!("page-value-{value}"))
    }
}

#[gpui_kit::test]
fn switching_keeps_page_entities_and_renders_only_the_active_page(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_, cx| MainView::new(cx));
    let page = cx.new(|_| StatefulPage(7));

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_tab("Dynamic page", page.clone(), cx)
        });
    });
    assert!(cx.debug_bounds("page-value-7").is_some());

    cx.update(|_, cx| {
        view.update(cx, |view, cx| view.select_tab(0, cx));
        page.update(cx, |page, cx| {
            page.0 = 42;
            cx.notify();
        });
    });
    assert!(cx.debug_bounds("page-value-42").is_none());

    cx.update(|_, cx| {
        view.update(cx, |view, cx| view.select_tab(1, cx));
    });
    assert!(cx.debug_bounds("page-value-42").is_some());
    cx.read(|cx| assert_eq!(view.read(cx).tabs[1].page.entity_id(), page.entity_id()));
}
