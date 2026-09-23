use std::{fs, sync::Arc};

use collection::{Method, Request};
use gpui_kit::TestAppContext;

use super::{
    editing_tests::{Fixture, sidebar},
    tree::ItemKind,
};

#[gpui_kit::test]
fn saving_body_keeps_tree_and_browsing_state(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    let path = fixture.0.join("API/Users/list.toml");

    sidebar.update(cx, |panel, cx| {
        panel.toggle(1, cx);
        panel.select_row(1, cx);
        let tree = panel.tree.clone();
        let visible = panel.visible.clone();
        let mut request = panel.collections.file(&path).unwrap().request.clone();
        let Request::Http(http) = &mut request;
        http.body = Some(b"updated body".to_vec());

        panel
            .save_request(&path, "list", request.clone(), cx)
            .unwrap();

        assert!(Arc::ptr_eq(&tree, &panel.tree));
        assert!(Arc::ptr_eq(&visible, &panel.visible));
        assert!(panel.collapsed.contains(&1));
        assert_eq!(panel.selected, Some(1));
        let Request::Http(cached) = &panel.collections.file(&path).unwrap().request;
        assert_eq!(cached.body.as_deref(), Some(b"updated body".as_slice()));
        let Request::Http(saved) = collection::FileEntry::from_path(&path).unwrap().request;
        assert_eq!(saved.body.as_deref(), Some(b"updated body".as_slice()));
    });
}

#[gpui_kit::test]
fn saving_updates_method_name_and_search_without_rebuilding_browsing_rows(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    let path = fixture.0.join("API/Users/list.toml");
    let browsing = cx.read(|cx| sidebar.read(cx).unfiltered_rows.clone().unwrap());
    let old_tree = cx.read(|cx| sidebar.read(cx).tree.clone());

    // A pending search for the previous URL must not overwrite the saved search results.
    sidebar.update(cx, |panel, cx| {
        panel.query = "/users".into();
        panel.refresh_rows(false, cx);
        panel.selected = Some(2);
        panel.collapsed.insert(1);
        let mut request = panel.collections.file(&path).unwrap().request.clone();
        let Request::Http(http) = &mut request;
        http.method = Method::Post;
        http.path = "/accounts".into();
        let contents = fs::read_to_string(&path)
            .unwrap()
            .replace("List users", "Create account");
        fs::write(&path, contents).unwrap();

        panel.save_request(&path, "list", request, cx).unwrap();
    });
    cx.run_until_parked();

    sidebar.update(cx, |panel, cx| {
        assert!(panel.visible.is_empty());
        assert_eq!(panel.selected, Some(2));
        assert_eq!(panel.selected_row, None);
        assert!(panel.collapsed.contains(&1));
        assert!(Arc::ptr_eq(
            &browsing,
            panel.unfiltered_rows.as_ref().unwrap()
        ));
        assert_eq!(panel.tree.items[2].kind, ItemKind::Request("POST"));
        assert_eq!(panel.tree.items[2].label, "Create account");
        assert_eq!(panel.tree.search.matching_rows("/accounts"), vec![2]);
        assert_eq!(panel.tree.search.matching_rows("create account"), vec![2]);
        assert!(panel.tree.search.matching_rows("GET").is_empty());
        assert_eq!(old_tree.search.matching_rows("/users"), vec![2]);
        assert!(old_tree.search.matching_rows("/accounts").is_empty());

        panel.query = "POST".into();
        panel.refresh_rows(false, cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let panel = sidebar.read(cx);
        assert_eq!(*panel.visible, vec![0, 1, 2]);
        assert_eq!(panel.selected_row, Some(2));
    });
}
