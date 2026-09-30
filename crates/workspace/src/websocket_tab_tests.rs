use std::fs;

use collection::CollectionRegistry;
use gpui_kit::{Modifiers, TestAppContext};

use crate::actions::NewWebSocketTab;
use crate::main_view::Page;
use crate::tests::{click, init, no_environments, workspace};

#[gpui_kit::test]
fn saved_websocket_opens_from_the_sidebar_and_saves_its_edits(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("Streams/prices.toml");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        &file,
        "id = \"prices\"\nname = \"Prices\"\nschema_version = 1\n[request]\ntype = \"websocket\"\nurl = \"wss://example.test/prices\"\nmessage = \"{\\\"subscribe\\\":true}\"\n",
    )
    .unwrap();
    let collections = CollectionRegistry::from_path(directory.path()).unwrap();

    init(cx);
    let (layout, cx) = workspace(collections, no_environments(), cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, _| window.refresh());
    let row = cx.debug_bounds("collection-row-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());

    let draft = cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].title, "Prices");
        assert_eq!(tabs.tabs[1].label, Some("WS"));
        assert_eq!(tabs.tabs[1].location(cx).unwrap().path, file);

        let draft = tabs.tabs[1].websocket();
        let data = draft.read(cx);
        assert_eq!(data.request.url, "wss://example.test/prices");
        assert_eq!(data.request.message, "{\"subscribe\":true}");
        assert!(!data.is_dirty());
        draft
    });
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("tab-method-2").is_some());
    assert!(cx.debug_bounds("websocket-draft").is_some());

    // Opening the row again selects the existing tab.
    cx.update(|window, _| window.refresh());
    let row = cx.debug_bounds("collection-row-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));

    click(cx, "websocket-url");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("wss://example.test/v2");
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("tab-dirty-2").is_some());

    cx.simulate_keystrokes("secondary-s");
    cx.read(|cx| assert!(!draft.read(cx).is_dirty()));
    let request::Request::WebSocket(saved) = collection::FileEntry::from_path(&file)
        .unwrap()
        .request
    else {
        panic!("expected a WebSocket request");
    };
    assert_eq!(saved.url, "wss://example.test/v2");
    assert_eq!(saved.message, "{\"subscribe\":true}");
}

#[gpui_kit::test]
fn a_new_websocket_tab_is_saved_through_the_dialog(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("Streams")).unwrap();
    let collections = CollectionRegistry::from_path(directory.path()).unwrap();

    init(cx);
    let (layout, cx) = workspace(collections, no_environments(), cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, cx| {
        tabs.update(cx, |tabs, cx| tabs.focus(window, cx));
        window.dispatch_action(Box::new(NewWebSocketTab), cx);
    });

    let draft = cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].label, Some("WS"));
        assert!(matches!(tabs.tabs[1].page, Page::WebSocket(_)));
        tabs.tabs[1].websocket()
    });

    click(cx, "websocket-url");
    cx.simulate_input("wss://example.test/feed");
    cx.simulate_keystrokes("secondary-s");
    dialog_click(cx, "save-request-name");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Feed");
    dialog_click(cx, "save-destination-0");
    dialog_click(cx, "confirm-save-request");

    let file = collection::FileEntry::from_path(directory.path().join("Streams/Feed.toml")).unwrap();
    let request::Request::WebSocket(saved) = file.request else {
        panic!("expected a WebSocket request");
    };
    cx.read(|cx| {
        assert_eq!(draft.read(cx).request, saved);
        assert!(!draft.read(cx).is_dirty());
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().collection,
            "Streams"
        );
    });
}

#[gpui_kit::test]
fn the_send_shortcut_connects_a_websocket(cx: &mut TestAppContext) {
    // The handshake is never answered, so the tab stays connecting.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();

    init(cx);
    let (layout, cx) = workspace(CollectionRegistry::new(), no_environments(), cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, cx| {
        tabs.update(cx, |tabs, cx| tabs.focus(window, cx));
        window.dispatch_action(Box::new(NewWebSocketTab), cx);
    });
    let draft = cx.read(|cx| tabs.read(cx).tabs[1].websocket());

    click(cx, "websocket-url");
    cx.simulate_input(&format!("ws://{}", listener.local_addr().unwrap()));
    cx.read(|cx| assert!(!draft.read(cx).is_connecting_for_test()));
    cx.simulate_keystrokes("secondary-enter");
    cx.read(|cx| assert!(draft.read(cx).is_connecting_for_test()));
}

/// Click in the save dialog once it is mounted and painted.
fn dialog_click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    click(cx, selector);
}
