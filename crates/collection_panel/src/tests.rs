use std::{
    collections::HashSet,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use collection::CollectionRegistry;
use gpui_kit::component::Root;
use gpui_kit::{AppContext, Focusable, Modifiers, TestAppContext, px, size};

use super::{
    CollectionPanel,
    tree::{CollectionTree, ItemKind},
};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn collections() -> CollectionRegistry {
    let directory = std::env::temp_dir().join(format!(
        "request-eagle-sidebar-test-{}-{}-{}",
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

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

    let result = CollectionRegistry::from_path(&directory);
    fs::remove_dir_all(directory).unwrap();

    result.unwrap()
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
fn sidebar_virtualizes_rows_and_handles_collapse_search_and_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
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

    cx.simulate_keystrokes("cmd-a backspace");
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

#[gpui_kit::test]
fn keyboard_can_tab_through_new_collection_into_the_tree(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
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

    cx.simulate_keystrokes("tab tab");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(sidebar.focus_handle(cx).is_focused(window));
    });
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(0)));
    cx.simulate_keystrokes("down");
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(1)));

    cx.simulate_keystrokes("shift-tab shift-tab");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn keyboard_can_enter_filtered_results_without_a_click(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
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

    cx.simulate_keystrokes("shift-tab shift-tab up");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.last().copied());
    });

    cx.simulate_keystrokes("shift-tab shift-tab enter");
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.selected, sidebar.visible.first().copied());
    });

    cx.simulate_keystrokes("shift-tab shift-tab left space right shift-up");
    cx.update(|window, cx| {
        let search = &sidebar.read(cx).search;
        assert!(search.focus_handle(cx).is_focused(window));
        assert_eq!(search.read(cx).value(), "/posts/ 7");
    });

    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("no-such-request");
    cx.run_until_parked();
    cx.simulate_keystrokes("down up enter");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).visible.is_empty());
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });
    cx.simulate_keystrokes("tab tab down up home end left right enter space shift-tab shift-tab");
    cx.update(|window, cx| {
        assert!(sidebar.read(cx).search.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn keyboard_browses_collections_from_initial_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
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
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        let editor = sidebar
            .rename
            .as_ref()
            .expect("Enter should rename the selected folder");
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
