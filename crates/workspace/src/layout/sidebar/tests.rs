use std::{collections::HashSet, path::Path, sync::Arc};

use collection::CollectionRegistry;
use gpui_kit::{Modifiers, TestAppContext, px, size};

use super::{
    Sidebar,
    tree::{CollectionTree, ItemKind},
};

fn fixtures() -> CollectionRegistry {
    CollectionRegistry::from_path(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/collections"),
    )
    .unwrap()
}

#[test]
fn real_fixtures_load_with_nested_requests_and_local_environments() {
    let collections = fixtures();
    let tree = CollectionTree::new(&collections);

    assert_eq!(tree.roots.len(), 3);
    assert_eq!(
        tree.roots
            .iter()
            .map(|&index| tree.items[index].request_count)
            .sum::<usize>(),
        23
    );
    assert_eq!(
        tree.items
            .iter()
            .filter(|item| item.kind == ItemKind::Request("POST"))
            .count(),
        3
    );
    assert!(
        tree.items
            .iter()
            .any(|item| item.depth == 3 && item.kind == ItemKind::Request("GET"))
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
        ["JSONPlaceholder", "Posts", "Comments", "List post comments"]
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
        cx.add_window_view(|window, cx| Sidebar::new(Arc::new(fixtures()), window, cx));
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

    let search = cx.read(|cx| sidebar.read(cx).search.clone());
    cx.update(|window, cx| search.update(cx, |input, cx| input.focus(window, cx)));
    cx.simulate_input("/delay/1");
    cx.run_until_parked();

    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.visible.len(), 3);
        assert_eq!(
            sidebar.tree.items[*sidebar.visible.last().unwrap()].label,
            "One second delay"
        );
    });

    cx.simulate_keystrokes("cmd-a backspace");
    cx.run_until_parked();
    cx.read(|cx| {
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
