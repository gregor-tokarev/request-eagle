use std::path::Path;

use gpui_kit::{
    AppContext, Context, Entity, Focusable, InputEvent as _, InteractiveElement, IntoElement,
    Modifiers, ParentElement, Render, ScrollDelta, ScrollWheelEvent, Styled, TestAppContext,
    TouchPhase, VisualTestContext, Window, div, point, px, size,
};

use super::main_view::MainView;
use crate::workspace::Layout;
use tab_ui::RequestDraft;

fn workspace(cx: &mut TestAppContext) -> (Entity<Layout>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
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
fn search_shortcut_focuses_sidebar_from_tree_and_request_inputs(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let draft = cx.read(|cx| {
        layout.read(cx).main_view.read(cx).tabs[0]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.update(|window, cx| {
        cx.set_reduce_motion(true);
        draft.update(cx, |draft, cx| {
            draft.set_method(collection::Method::Post, cx);
            draft.prepare(window, cx);
        });
    });

    let body_tab = cx.debug_bounds("request-section-Body").unwrap();
    cx.simulate_click(body_tab.center(), Modifiers::default());

    let search = cx.debug_bounds("collections-search").unwrap();
    cx.simulate_click(search.center(), Modifiers::default());
    let search_focus = cx.update(|window, cx| window.focused(cx).unwrap());
    cx.simulate_input("old query");

    cx.update(|window, cx| {
        window.focus(&layout.read(cx).sidebar.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("secondary-f");
    cx.update(|window, _| assert!(search_focus.is_focused(window)));

    for selector in ["request-url", "request-body", "collections-search"] {
        let bounds = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.simulate_keystrokes("secondary-f");
        cx.update(|window, _| assert!(search_focus.is_focused(window), "{selector}"));
    }

    // Repeated search selects the existing query for replacement.
    cx.simulate_input("new query");
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("new query")
    );

    cx.update(|window, cx| {
        window.focus(&draft.read(cx).url_input().unwrap().focus_handle(cx), cx);
        layout.update(cx, |layout, cx| layout.toggle_sidebar(cx));
    });
    cx.read(|cx| assert!(!*layout.read(cx).sidebar_visible.read(cx)));

    cx.simulate_keystrokes("secondary-f");
    cx.update(|window, cx| {
        assert!(*layout.read(cx).sidebar_visible.read(cx));
        assert!(search_focus.is_focused(window));
    });

    // Escape leaves search even when the filter has no results.
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        assert!(!search_focus.is_focused(window));
        assert!(layout.read(cx).sidebar.focus_handle(cx).is_focused(window));
    });

    cx.simulate_keystrokes("secondary-f end");
    cx.simulate_input("fresh query");
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("fresh query")
    );
}

#[gpui_kit::test]
fn new_tabs_start_as_independent_empty_get_requests(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    let first = cx.read(|cx| {
        let view = view.read(cx);
        let tab = &view.tabs[0];

        assert_eq!(tab.badge.map(|badge| badge.label), Some("GET"));
        assert!(tab.request_path.is_none());

        tab.page.view().downcast::<RequestDraft>().ok().unwrap()
    });

    assert!(cx.debug_bounds("request-draft").is_some());
    assert!(cx.debug_bounds("request-collection").is_none());
    assert!(cx.debug_bounds("tab-method-1").is_some());
    cx.read(|cx| {
        let draft = first.read(cx);

        assert!(matches!(draft.request.method, collection::Method::Get));
        assert!(draft.request.path.is_empty());
        assert!(draft.request.headers.is_empty());
        assert!(draft.request.body.is_none());
        assert!(draft.request.query.is_none());
        assert!(draft.url_input().unwrap().read(cx).value().is_empty());
    });

    let url = cx.debug_bounds("request-url").unwrap();
    cx.simulate_click(url.center(), Modifiers::default());
    cx.simulate_input("https://example.com/first");
    cx.read(|cx| assert_eq!(first.read(cx).request.path, "https://example.com/first"));

    // Creating a tab while editing a URL must not inherit that draft's data.
    cx.simulate_keystrokes("secondary-t");
    cx.read(|cx| {
        let view = view.read(cx);
        let tab = &view.tabs[1];
        let draft = tab.page.view().downcast::<RequestDraft>().ok().unwrap();

        assert_eq!(view.selected, Some(1));
        assert_eq!(tab.badge.map(|badge| badge.label), Some("GET"));
        assert!(tab.request_path.is_none());
        assert_ne!(draft, first);
        assert!(draft.read(cx).request.path.is_empty());
        assert!(
            draft
                .read(cx)
                .url_input()
                .unwrap()
                .read(cx)
                .value()
                .is_empty()
        );
    });

    cx.simulate_keystrokes("secondary-1");
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[0].page.view().entity_id(),
            first.entity_id()
        );
        assert_eq!(
            first.read(cx).url_input().unwrap().read(cx).value(),
            "https://example.com/first"
        );
    });
}

#[gpui_kit::test]
fn plus_button_does_not_assign_a_new_request_to_the_active_collection(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    let saved_path = Path::new("/collection/saved.toml");

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_request(
                saved_path,
                "saved".into(),
                "Saved request".into(),
                "Collection".into(),
                Vec::new(),
                &collection::HttpRequest {
                    method: collection::Method::Post,
                    ..Default::default()
                }
                .into(),
                cx,
            );
        });
    });

    let new_tab = cx.debug_bounds("new-tab").unwrap();
    cx.simulate_click(new_tab.center(), Modifiers::default());
    cx.run_until_parked();
    cx.read(|cx| {
        let view = view.read(cx);
        let tab = &view.tabs[2];
        let draft = tab.page.view().downcast::<RequestDraft>().ok().unwrap();

        assert_eq!(view.selected, Some(2));
        assert_eq!(tab.badge.map(|badge| badge.label), Some("GET"));
        assert!(tab.request_path.is_none());
        assert!(matches!(
            draft.read(cx).request.method,
            collection::Method::Get
        ));
        assert!(draft.read(cx).request.path.is_empty());
        assert_eq!(view.tabs[1].request_path.as_deref(), Some(saved_path));
        assert_eq!(view.tabs[1].badge.map(|badge| badge.label), Some("POST"));
    });
    // A resizer's settling frame can replay the cached page. Refresh before
    // inspecting debug selectors, which are collected during a fresh layout.
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("request-draft").is_some());
}

#[gpui_kit::test]
fn request_editor_preserves_fields_and_method_without_assigning_a_collection(
    cx: &mut TestAppContext,
) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    let draft = cx.read(|cx| {
        view.read(cx).tabs[0]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });

    for (selector, value) in [
        ("headers-key-0", "Accept"),
        ("headers-value-0", "application/json"),
    ] {
        let field = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(field.center(), Modifiers::default());
        cx.simulate_input(value);
    }
    assert!(cx.debug_bounds("headers-key-1").is_some());

    let params = cx.debug_bounds("request-section-Params").unwrap();
    cx.simulate_click(params.center(), Modifiers::default());
    for (selector, value) in [("params-key-0", "page"), ("params-value-0", "2")] {
        let field = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(field.center(), Modifiers::default());
        cx.simulate_input(value);
    }

    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(collection::Method::Post, cx)
        });
    });

    cx.update(|window, _| window.refresh());
    let body = cx.debug_bounds("request-section-Body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    let body = cx.debug_bounds("request-body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.simulate_input("hello");
    let params = cx.debug_bounds("request-section-Params").unwrap();
    cx.simulate_click(params.center(), Modifiers::default());

    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(collection::Method::Post, cx)
        })
    });
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[0].badge.map(|badge| badge.label),
            Some("POST")
        );
        assert!(view.read(cx).tabs[0].request_path.is_none());
        assert_eq!(
            draft.read(cx).request.headers,
            [("Accept".into(), "application/json".into())]
        );
        assert_eq!(
            draft.read(cx).request.query,
            Some(vec![("page".into(), "2".into())])
        );
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"hello".as_slice())
        );
    });

    cx.simulate_keystrokes("secondary-t");
    cx.read(|cx| {
        let view = view.read(cx);
        let new_draft = view.tabs[1]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();

        assert_eq!(view.tabs[1].badge.map(|badge| badge.label), Some("GET"));
        assert!(new_draft.read(cx).request.headers.is_empty());
        assert!(new_draft.read(cx).request.query.is_none());
        assert!(new_draft.read(cx).request.body.is_none());
    });

    cx.simulate_keystrokes("secondary-1");
    assert!(cx.debug_bounds("params-key-1").is_some());
    cx.read(|cx| {
        assert_eq!(draft.read(cx).request.method, collection::Method::Post);
        assert_eq!(draft.read(cx).request.headers[0].1, "application/json");
    });
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
        assert_eq!(
            view.read(cx).tabs[0].badge.map(|badge| badge.label),
            Some("GET")
        );
        assert!(view.read(cx).tabs[0].request_path.is_none());
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

    // Opening tabs in the background must not register thousands of input
    // listeners before those editors have ever been displayed.
    cx.read(|cx| {
        let unseen = view.read(cx).tabs[500]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        assert!(unseen.read(cx).url_input().is_none());
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
        preferences::init(cx);
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
    let second_request = cx.debug_bounds("collection-row-3").unwrap();
    cx.simulate_click(first_request.center(), Modifiers::default());
    let first_page = cx.read(|cx| {
        let view = view.read(cx);

        assert_eq!(view.tabs.len(), 2);
        assert_eq!(view.selected, Some(1));
        assert_eq!(view.tabs[1].title, "Get resource 0");
        assert_eq!(view.tabs[1].badge.map(|badge| badge.label), Some("GET"));

        view.tabs[1].page.view().entity_id()
    });

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
        assert_eq!(view.read(cx).tabs[1].page.view().entity_id(), first_page);
    });

    cx.simulate_keystrokes("secondary-w");
    cx.simulate_click(first_request.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(2));
        assert_eq!(view.read(cx).tabs[2].title, "Get resource 0");
        assert_ne!(view.read(cx).tabs[2].page.view().entity_id(), first_page);
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
            view.open_request(
                first_path,
                "first".into(),
                "Same name".into(),
                "Collection".into(),
                Vec::new(),
                &collection::HttpRequest::default().into(),
                cx,
            );
            view.open_request(
                second_path,
                "second".into(),
                "Same name".into(),
                "Collection".into(),
                Vec::new(),
                &collection::HttpRequest {
                    method: collection::Method::Post,
                    ..Default::default()
                }
                .into(),
                cx,
            );
        });
    });
    let first_page = cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);

        view.read(cx).tabs[1].page.view().entity_id()
    });

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.open_request(
                first_path,
                "first".into(),
                "Renamed request".into(),
                "Renamed collection".into(),
                Vec::new(),
                &collection::HttpRequest {
                    method: collection::Method::Put,
                    ..Default::default()
                }
                .into(),
                cx,
            );
        });
    });
    cx.read(|cx| {
        assert_eq!(view.read(cx).tabs.len(), 3);
        assert_eq!(view.read(cx).selected, Some(1));
        assert_eq!(view.read(cx).tabs[1].title, "Renamed request");
        assert_eq!(
            view.read(cx).tabs[1].badge.map(|badge| badge.label),
            Some("GET")
        );
        assert_eq!(view.read(cx).tabs[1].page.view().entity_id(), first_page);
        let draft = view.read(cx).tabs[1]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        assert_eq!(draft.read(cx).name, "Renamed request");
        assert_eq!(
            draft.read(cx).collection.as_deref(),
            Some("Renamed collection")
        );
        assert_eq!(view.read(cx).tabs[2].title, "Same name");
        assert_eq!(
            view.read(cx).tabs[2].badge.map(|badge| badge.label),
            Some("POST")
        );
    });
}

struct StatefulPage(usize);

impl tab_ui::TabPage for StatefulPage {}

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
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[1].page.view().entity_id(),
            page.entity_id()
        )
    });
}

struct RenderCountPage {
    renders: usize,
    child: Entity<StatefulPage>,
}

impl tab_ui::TabPage for RenderCountPage {}

impl Render for RenderCountPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;

        div().size_full().child(self.child.clone())
    }
}

#[gpui_kit::test]
fn scrolling_tabs_reuses_the_page_but_child_changes_and_resize_redraw_it(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    let child = cx.new(|_| StatefulPage(7));
    let page = cx.new(|_| RenderCountPage {
        renders: 0,
        child: child.clone(),
    });

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for _ in 0..30 {
                view.new_tab(cx);
            }
            view.open_tab("Counted page", page.clone(), cx);
        });
    });

    let renders = cx.read(|cx| page.read(cx).renders);
    let bar = cx.debug_bounds("main-tab-bar").unwrap();
    cx.update(|window, cx| {
        window.dispatch_event(
            ScrollWheelEvent {
                position: bar.center(),
                delta: ScrollDelta::Pixels(point(px(1800.), px(0.))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("page-tab-32").is_none());
    cx.read(|cx| assert_eq!(page.read(cx).renders, renders));

    cx.update(|_, cx| {
        child.update(cx, |child, cx| {
            child.0 = 42;
            cx.notify();
        })
    });
    assert!(cx.debug_bounds("page-value-42").is_some());
    let renders = cx.read(|cx| page.read(cx).renders);

    cx.simulate_resize(size(px(1440.), px(900.)));
    cx.read(|cx| assert!(page.read(cx).renders > renders));
    assert!(cx.debug_bounds("page-value-42").is_some());
}

#[derive(Default)]
struct ProtocolPage {
    prepared: bool,
    sends: usize,
}

impl tab_ui::TabPage for ProtocolPage {
    fn tab_state(&self) -> tab_ui::TabState {
        tab_ui::TabState {
            badge: Some(tab_ui::TabBadge {
                label: "RPC",
                tone: tab_ui::TabBadgeTone::Info,
            }),
            dirty: self.sends > 0,
        }
    }

    fn prepare(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.prepared = true;
    }

    fn send(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.sends += 1;
        cx.notify();
    }
}

impl Render for ProtocolPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Protocol page")
    }
}

#[gpui_kit::test]
fn non_http_pages_receive_tab_actions_and_report_unsaved_changes(cx: &mut TestAppContext) {
    let (layout, cx) = workspace(cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    let page = cx.new(|_| ProtocolPage::default());

    cx.update(|window, cx| {
        tabs.update(cx, |tabs, cx| {
            tabs.open_tab("Protocol page", page.clone(), cx);
            tabs.focus(window, cx);
        });
    });
    cx.read(|cx| assert!(page.read(cx).prepared));
    cx.simulate_keystrokes("secondary-enter");
    cx.read(|cx| {
        assert_eq!(page.read(cx).sends, 1);
        assert_eq!(
            tabs.read(cx).tabs[1].badge.map(|badge| badge.label),
            Some("RPC")
        );
    });
    assert!(cx.debug_bounds("tab-dirty-2").is_some());

    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));

    let passive_page = cx.new(|_| StatefulPage(7));
    cx.update(|window, cx| {
        tabs.update(cx, |tabs, cx| {
            tabs.open_tab("Collection page", passive_page, cx);
            tabs.focus(window, cx);
        });
    });
    cx.simulate_keystrokes("secondary-enter");
    cx.read(|cx| {
        assert_eq!(page.read(cx).sends, 1);
        assert_eq!(tabs.read(cx).tabs[1].badge.map(|badge| badge.label), None);
    });
    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
}
