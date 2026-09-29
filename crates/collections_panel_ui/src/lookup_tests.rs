use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gpui_kit::{Entity, TestAppContext, VisualTestContext};

use super::{CollectionPanel, CollectionPanelEvent, RequestMatch, tests::collections};

fn find_requests(
    sidebar: &Entity<CollectionPanel>,
    query: &str,
    limit: usize,
    cx: &mut VisualTestContext,
) -> Vec<RequestMatch> {
    let results = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let task = sidebar.read(cx).find_requests(query, limit, cx);
        let results = results.clone();
        cx.spawn(async move |_| *results.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();

    results.take().expect("request search should finish")
}

#[gpui_kit::test]
fn finds_collections_and_requests_by_name(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));

    cx.read(|cx| {
        let sidebar = sidebar.read(cx);

        let collections = sidebar.find_collections("", 10);
        let names: Vec<_> = collections.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["Example API", "Status API"]);
        assert_eq!(collections[0].request_count, 25);

        let collections = sidebar.find_collections("status", 10);
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "Status API");

        // Limits apply to collections, not only requests.
        assert_eq!(sidebar.find_collections("", 1).len(), 1);
    });

    assert!(find_requests(&sidebar, "", 10, cx).is_empty());

    let requests = find_requests(&sidebar, "COMMENT", 10, cx);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].name, "Create comment");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].location, "Example API › Posts › Comments");

    // Requests follow the sidebar filter, which also matches URLs.
    let requests = find_requests(&sidebar, "/status/404", 10, cx);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].name, "Not found");

    assert_eq!(find_requests(&sidebar, "get post", 3, cx).len(), 3);
}

#[gpui_kit::test]
fn opens_collections_and_requests_by_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (sidebar, cx) =
        cx.add_window_view(|window, cx| CollectionPanel::new(collections(), window, cx));

    let opened = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
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

    let request = find_requests(&sidebar, "Not found", 1, cx)[0].path.clone();
    let status = cx.read(|cx| {
        sidebar.read(cx).find_collections("Status", 1)[0]
            .path
            .clone()
    });

    cx.update(|_, cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.open_at(&request, cx);
            sidebar.open_at(&status, cx);
            // Paths outside the tree open nothing.
            sidebar.open_at(&status.join("missing.toml"), cx);
        })
    });
    assert_eq!(*opened.borrow(), [request, status]);
}
