use std::{cell::RefCell, collections::HashSet, fs, path::PathBuf, rc::Rc, sync::Arc};

use collection::CollectionRegistry;
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext, Entity, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, point, px, size,
};

use super::{
    CollectionPanel, CollectionPanelEvent,
    tree::{CollectionTree, ItemKind},
};

pub(super) fn collections() -> CollectionRegistry {
    let directory = tempfile::tempdir().unwrap();
    let directory = directory.path();

    for name in ["Example API", "Status API"] {
        let collection = directory.join(name);
        fs::create_dir_all(&collection).unwrap();
        fs::write(
            collection.join("environment.toml"),
            "base_url = \"https://example.com\"\n",
        )
        .unwrap();
    }

    let mut requests: Vec<_> = (0..24)
        .map(|index| {
            (
                format!("Example API/Posts/request-{index:02}.toml"),
                format!("Get post {index}"),
                "GET",
                format!("/posts/{index}"),
            )
        })
        .collect();
    requests.extend([
        (
            "Example API/Posts/Comments/create.toml".into(),
            "Create comment".into(),
            "POST",
            "/comments".into(),
        ),
        (
            "Status API/Responses/not-found.toml".into(),
            "Not found".into(),
            "GET",
            "/status/404".into(),
        ),
    ]);

    for (index, (file, name, method, path)) in requests.into_iter().enumerate() {
        let file = directory.join(file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, format!(
            "id = \"request-{index}\"\nname = \"{name}\"\nschema_version = 1\n\n[request]\ntype = \"http\"\nmethod = \"{method}\"\npath = \"{path}\"\n"
        )).unwrap();
    }

    CollectionRegistry::from_path(directory)
}

#[test]
fn tree_preserves_hierarchy_and_filters_collapsed_collections() {
    let collections = collections();
    let tree = CollectionTree::new(&collections);

    assert_eq!(tree.roots.len(), 2);
    assert_eq!(
        tree.roots
            .iter()
            .map(|&index| tree.items[index].request_count)
            .sum::<usize>(),
        26
    );
    assert_eq!(
        tree.items
            .iter()
            .filter(|item| item.kind == ItemKind::Request("POST"))
            .count(),
        1
    );
    assert!(
        tree.items
            .iter()
            .any(|item| item.depth == 3 && item.kind == ItemKind::Request("POST"))
    );
    assert!(
        !tree
            .items
            .iter()
            .any(|item| item.label == "environment.toml")
    );

    let comment = tree
        .items
        .iter()
        .position(|item| item.kind == ItemKind::Request("POST"))
        .unwrap();
    assert_eq!(
        tree.location(comment),
        (
            "Example API".into(),
            vec!["Posts".into(), "Comments".into()]
        )
    );
    assert_eq!(tree.location(tree.roots[1]), ("Status API".into(), vec![]));

    for collection in collections.collections() {
        assert!(
            collection
                .local_env()
                .resolve("base_url")
                .unwrap()
                .starts_with("https://")
        );
    }

    let mut collapsed: HashSet<_> = tree.roots.iter().copied().collect();
    assert_eq!(tree.visible_rows(&collapsed, ""), tree.roots);

    let rows = tree.visible_rows(&collapsed, "comments");
    let labels: Vec<_> = rows
        .iter()
        .map(|&index| tree.items[index].label.as_ref())
        .collect();
    assert_eq!(
        labels,
        ["Example API", "Posts", "Comments", "Create comment"]
    );

    let rows = tree.visible_rows(&collapsed, "/status/404");
    assert_eq!(tree.items[*rows.last().unwrap()].label, "Not found");
    assert!(tree.visible_rows(&collapsed, "no-such-request").is_empty());

    collapsed.clear();
    assert_eq!(tree.visible_rows(&collapsed, "").len(), tree.items.len());
}

#[gpui_kit::test]
fn files_that_could_not_be_loaded_are_listed(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("API")).unwrap();
    for index in 0..40 {
        fs::write(
            directory.path().join(format!("API/Broken {index:02}.toml")),
            "id = ",
        )
        .unwrap();
    }
    let collections = CollectionRegistry::from_path(directory.path());

    let mut panel = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CollectionPanel::new(collections, window, cx));
        panel = Some(view.clone());

        Root::new(view, window, cx)
    });
    let panel = panel.unwrap();
    cx.simulate_resize(size(px(300.), px(600.)));
    cx.run_until_parked();

    assert!(cx.debug_bounds("skipped-file-0").is_some());
    assert!(cx.debug_bounds("skipped-file-39").is_some());
    // The list scrolls rather than pushing the collection out of view.
    assert!(cx.debug_bounds("collections-skipped").unwrap().size.height < px(300.));
    assert!(cx.debug_bounds("collection-row-0").unwrap().top() < px(600.));

    // The keyboard reaches the files after the search field, and they count
    // as the panel's focus.
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.focus_search(window, cx)));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = panel.read(cx);
        assert!(!panel.search.focus_handle(cx).is_focused(window));
        assert!(panel.contains_focus(window, cx));
    });
}

#[gpui_kit::test]
fn sidebar_virtualizes_rows_and_handles_collapse_search_and_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));
    cx.simulate_resize(size(px(300.), px(500.)));
    cx.run_until_parked();

    let last = cx.read(|cx| sidebar.read(cx).tree.items.len() - 1);
    let last_selector: &'static str = Box::leak(format!("collection-row-{last}").into_boxed_str());
    assert!(cx.debug_bounds("collection-row-0").is_some());
    assert!(
        cx.debug_bounds(last_selector).is_none(),
        "offscreen rows must not be rendered"
    );

    let root = cx.debug_bounds("collection-row-0").unwrap();
    cx.simulate_click(root.center(), Modifiers::default());
    cx.run_until_parked();

    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert!(sidebar.collapsed.contains(&0));
        assert_eq!(sidebar.selected, Some(0));
        assert!(!sidebar.visible.contains(&1));
    });

    let browsing_rows = cx.read(|cx| sidebar.read(cx).visible.clone());
    let search = cx.read(|cx| sidebar.read(cx).search.clone());
    cx.update(|window, cx| search.update(cx, |input, cx| input.focus(window, cx)));
    cx.simulate_input("/posts/7");
    cx.run_until_parked();

    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.visible.len(), 3);
        assert_eq!(
            sidebar.tree.items[*sidebar.visible.last().unwrap()].label,
            "Get post 7"
        );
    });

    cx.simulate_keystrokes("secondary-a backspace");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            Arc::ptr_eq(&sidebar.read(cx).visible, &browsing_rows),
            "clearing search must reuse the browsing rows"
        );
        assert!(
            !sidebar.read(cx).visible.contains(&1),
            "clearing search preserves collapsed folders"
        )
    });

    let root = cx.debug_bounds("collection-row-0").unwrap();
    cx.simulate_click(root.center(), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(1)));
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(last)));
    assert!(cx.debug_bounds(last_selector).is_some());
}

type Opened = Rc<RefCell<Vec<PathBuf>>>;

fn clicking_sidebar(
    cx: &mut TestAppContext,
) -> (Entity<CollectionPanel>, Opened, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));
    cx.simulate_resize(size(px(300.), px(500.)));
    cx.update(|window, _| window.activate_window());

    let opened = Opened::default();
    cx.update(|_, cx| {
        let opened = opened.clone();
        cx.subscribe(
            &sidebar,
            move |_, event: &CollectionPanelEvent, _| match event {
                CollectionPanelEvent::OpenRequest { path, .. }
                | CollectionPanelEvent::OpenCollection { path, .. } => {
                    opened.borrow_mut().push(path.clone())
                }
                _ => {}
            },
        )
        .detach();
    });

    (sidebar, opened, cx)
}

fn row_selector(index: usize) -> &'static str {
    Box::leak(format!("collection-row-{index}").into_boxed_str())
}

fn click_at(cx: &mut VisualTestContext, position: Point<Pixels>, click_count: usize) {
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position,
        click_count,
        ..Default::default()
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position,
        click_count,
        ..Default::default()
    });
    cx.run_until_parked();
}

/// Scroll to the bottom, where collapsing the last collection scrolls the
/// rows above it under the pointer, and return that collection's row centre.
fn last_collection(
    sidebar: &Entity<CollectionPanel>,
    cx: &mut VisualTestContext,
) -> (PathBuf, &'static str, Point<Pixels>) {
    cx.update(|window, cx| {
        let focus = sidebar.read(cx).focus.clone();
        focus.focus(window, cx);
    });
    cx.simulate_keystrokes("end");
    cx.run_until_parked();

    let (index, path) = cx.read(|cx| {
        let tree = &sidebar.read(cx).tree;
        let index = tree
            .items
            .iter()
            .position(|item| item.label == "Status API")
            .unwrap();
        (index, tree.items[index].path.clone())
    });
    let selector = row_selector(index);

    (path, selector, cx.debug_bounds(selector).unwrap().center())
}

#[gpui_kit::test]
fn quick_clicks_keep_toggling_the_collection_they_started_on(cx: &mut TestAppContext) {
    let (sidebar, opened, cx) = clicking_sidebar(cx);
    let (path, selector, position) = last_collection(&sidebar, cx);
    let index = cx.read(|cx| sidebar.read(cx).tree.index_of(&path).unwrap());

    click_at(cx, position, 1);
    assert_eq!(*opened.borrow(), std::slice::from_ref(&path));
    cx.read(|cx| assert!(sidebar.read(cx).collapsed.contains(&index)));
    assert!(!cx.debug_bounds(selector).unwrap().contains(&position));

    // Presses inside the double click interval toggle the collection again
    // rather than wait, and never reach the row collapsing moved under the
    // pointer.
    click_at(cx, position, 2);
    cx.read(|cx| assert!(!sidebar.read(cx).collapsed.contains(&index)));

    click_at(cx, position, 3);
    cx.read(|cx| assert!(sidebar.read(cx).collapsed.contains(&index)));
    assert_eq!(*opened.borrow(), [path.clone(), path.clone(), path]);
}

#[gpui_kit::test]
fn quick_clicks_toggle_a_folder_each_time(cx: &mut TestAppContext) {
    let (sidebar, _, cx) = clicking_sidebar(cx);
    let index = cx.read(|cx| {
        let items = &sidebar.read(cx).tree.items;
        items
            .iter()
            .position(|item| item.kind == ItemKind::Folder)
            .unwrap()
    });
    let position = cx.debug_bounds(row_selector(index)).unwrap().center();

    click_at(cx, position, 1);
    cx.read(|cx| assert!(sidebar.read(cx).collapsed.contains(&index)));

    click_at(cx, position, 2);
    cx.read(|cx| assert!(!sidebar.read(cx).collapsed.contains(&index)));
}

#[gpui_kit::test]
fn quick_clicks_that_reach_another_row_act_on_that_row(cx: &mut TestAppContext) {
    let (sidebar, opened, cx) = clicking_sidebar(cx);

    // Across the edge between two requests.
    let (index, paths) = cx.read(|cx| {
        let items = &sidebar.read(cx).tree.items;
        let index = (0..items.len() - 1)
            .find(|&index| {
                matches!(items[index].kind, ItemKind::Request(_))
                    && matches!(items[index + 1].kind, ItemKind::Request(_))
            })
            .unwrap();
        (
            index,
            vec![items[index].path.clone(), items[index + 1].path.clone()],
        )
    });
    let first = cx.debug_bounds(row_selector(index)).unwrap();
    let second = cx.debug_bounds(row_selector(index + 1)).unwrap();
    click_at(cx, point(first.center().x, first.bottom() - px(1.)), 1);
    click_at(cx, point(second.center().x, second.top() + px(1.)), 2);
    assert_eq!(*opened.borrow(), paths);

    // Scrolling after collapsing a collection leaves the next click on
    // the row now under the pointer.
    let (collection, _, position) = last_collection(&sidebar, cx);
    let before = opened.borrow().len();
    click_at(cx, position, 1);
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(32.))),
        ..Default::default()
    });
    cx.run_until_parked();
    let visible = cx.read(|cx| sidebar.read(cx).visible.clone());
    let index = visible
        .iter()
        .copied()
        .find(|&index| {
            cx.debug_bounds(row_selector(index))
                .is_some_and(|bounds| bounds.contains(&position))
        })
        .unwrap();
    let under = cx.read(|cx| sidebar.read(cx).tree.items[index].path.clone());
    assert_ne!(under, collection);

    click_at(cx, position, 2);
    assert_eq!(opened.borrow()[before..], [collection, under]);
}

#[gpui_kit::test]
fn keyboard_can_tab_from_search_into_the_tree(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });

    let mut sidebar = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CollectionPanel::new(collections(), window, cx));
        sidebar = Some(view.clone());

        Root::new(view, window, cx)
    });
    let sidebar = sidebar.unwrap();

    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });

    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(sidebar.focus_handle(cx).is_focused(window));
    });
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(0)));
    cx.simulate_keystrokes("down");
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(1)));

    cx.simulate_keystrokes("shift-tab");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn keyboard_can_enter_filtered_results_without_a_click(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });

    let mut sidebar = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CollectionPanel::new(collections(), window, cx));
        sidebar = Some(view.clone());

        Root::new(view, window, cx)
    });
    let sidebar = sidebar.unwrap();

    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.simulate_input("/posts/7");
    cx.run_until_parked();
    cx.simulate_keystrokes("down down down");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        let selected = sidebar.selected.expect("arrow keys should select a result");

        assert_eq!(sidebar.tree.items[selected].label, "Get post 7");
    });

    cx.simulate_keystrokes("shift-tab up");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.last().copied());
    });

    cx.simulate_keystrokes("shift-tab enter");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.first().copied());
    });

    cx.simulate_keystrokes("shift-tab left space right shift-up");
    cx.update(|window, cx| {
        let search = &sidebar.read(cx).search;
        assert!(search.focus_handle(cx).is_focused(window));
        assert_eq!(search.read(cx).value(), "/posts/ 7");
    });

    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("no-such-request");
    cx.run_until_parked();
    cx.simulate_keystrokes("down up enter");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).visible.is_empty());
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });
    cx.simulate_keystrokes("tab down up home end left right enter space shift-tab");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });

    for query in ["no-such-request", "/posts/7"] {
        cx.update(|window, cx| sidebar.update(cx, |sidebar, cx| sidebar.focus_search(window, cx)));
        cx.simulate_input(query);
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();

        cx.update(|window, cx| {
            let sidebar = sidebar.read(cx);

            assert!(sidebar.focus_handle(cx).is_focused(window));
            assert!(sidebar.search.read(cx).value().is_empty());
            assert!(sidebar.query.is_empty());
            assert_eq!(sidebar.visible.len(), sidebar.tree.items.len());
        });
    }
}

#[gpui_kit::test]
fn keyboard_browses_collections_from_initial_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
    });

    let mut sidebar = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CollectionPanel::new(collections(), window, cx));
        window.focus(&view.focus_handle(cx), cx);
        sidebar = Some(view.clone());

        Root::new(view, window, cx)
    });
    let sidebar = sidebar.unwrap();

    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(sidebar.focus_handle(cx).is_focused(window));
        assert_eq!(sidebar.read(cx).selected, Some(0));
    });

    cx.simulate_keystrokes("down right right");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(
            sidebar.tree.items[sidebar.selected.unwrap()].label,
            "Create comment"
        );
    });
    cx.simulate_keystrokes("left left");
    cx.run_until_parked();
    cx.read(|cx| assert!(sidebar.read(cx).collapsed.contains(&2)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(sidebar.read_with(cx, |sidebar, _| sidebar.rename.is_none()));
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        let editor = sidebar
            .rename
            .as_ref()
            .expect("F2 should rename the selected folder");
        assert_eq!(editor.input.read(cx).value(), "Comments");
        assert!(sidebar.collapsed.contains(&2));
    });
    cx.simulate_keystrokes("escape right");
    cx.run_until_parked();
    cx.read(|cx| assert!(!sidebar.read(cx).collapsed.contains(&2)));
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    cx.read(|cx| assert!(sidebar.read(cx).collapsed.contains(&2)));

    cx.simulate_keystrokes("home");
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(0)));
    cx.simulate_keystrokes("end");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.last().copied());
    });

    cx.simulate_keystrokes("shift-up cmd-home");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.last().copied());
    });
}
