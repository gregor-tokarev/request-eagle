use std::path::PathBuf;

use gpui_kit::{Focusable, TestAppContext};

use super::{CollectionPanel, CollectionPanelEvent, tests::collections};

#[gpui_kit::test]
fn finds_collections_requests_and_environments_by_name(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));

    cx.read(|cx| {
        let sidebar = sidebar.read(cx);

        let collections = sidebar.find_collections("");
        let names: Vec<_> = collections.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["Example API", "Status API"]);
        assert_eq!(collections[0].request_count, 25);

        let collections = sidebar.find_collections("status");
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "Status API");

        assert!(sidebar.find_requests("", 10).is_empty());

        let requests = sidebar.find_requests("COMMENT", 10);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].name, "Create comment");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].location, "Example API › Posts › Comments");

        // Requests follow the sidebar filter, which also matches URLs.
        let requests = sidebar.find_requests("/status/404", 10);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].name, "Not found");

        assert_eq!(sidebar.find_requests("get post", 3).len(), 3);

        let environments = sidebar.find_environments("");
        assert_eq!(environments.len(), 2);
        assert_eq!(environments[0].name, "Example API");
        assert_eq!(environments[0].variable_count, 1);
        assert!(
            environments[0]
                .path
                .ends_with("Example API/environment.toml")
        );

        // Variable names find the environment that defines them.
        assert_eq!(sidebar.find_environments("BASE_URL").len(), 2);
        assert!(sidebar.find_environments("token").is_empty());
    });
}

#[gpui_kit::test]
fn reveals_and_opens_rows_by_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));

    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<PathBuf>::new()));
    cx.update(|_, cx| {
        let opened = opened.clone();
        cx.subscribe(&sidebar, move |_, event: &CollectionPanelEvent, _| {
            if let CollectionPanelEvent::OpenRequest { path, .. } = event {
                opened.borrow_mut().push(path.clone());
            }
        })
        .detach();
    });

    let request = cx.read(|cx| {
        sidebar.read(cx).find_requests("Not found", 1)[0]
            .path
            .clone()
    });
    let status = cx.read(|cx| sidebar.read(cx).find_collections("Status")[0].path.clone());

    // Filter and collapse the tree so the target row starts hidden.
    let search = cx.read(|cx| sidebar.read(cx).search.clone());
    cx.update(|window, cx| search.update(cx, |input, cx| input.focus(window, cx)));
    cx.simulate_input("comment");
    cx.run_until_parked();
    cx.update(|_, cx| {
        sidebar.update(cx, |sidebar, cx| {
            let index = sidebar.tree.roots[1];
            sidebar.collapsed.insert(index);
            sidebar.unfiltered_rows = None;
            sidebar.refresh_rows(false, cx);
        })
    });
    cx.run_until_parked();

    cx.update(|window, cx| sidebar.update(cx, |sidebar, cx| sidebar.reveal(&request, window, cx)));
    cx.run_until_parked();

    cx.update(|window, cx| {
        let panel = sidebar.read(cx);
        let selected = panel.selected.unwrap();

        assert_eq!(panel.tree.items[selected].path, request);
        assert!(panel.visible.contains(&selected));
        assert!(panel.query.is_empty());
        assert!(panel.search.read(cx).value().is_empty());
        assert!(panel.focus_handle(cx).is_focused(window));
    });

    cx.update(|window, cx| sidebar.update(cx, |sidebar, cx| sidebar.reveal(&status, window, cx)));
    cx.read(|cx| {
        let panel = sidebar.read(cx);
        assert_eq!(panel.tree.items[panel.selected.unwrap()].path, status);
    });

    cx.update(|_, cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.open_request_at(&request, cx);
            // Collection rows have no page to open.
            sidebar.open_request_at(&status, cx);
        })
    });
    assert_eq!(*opened.borrow(), [request]);
}
