use std::{
    fs,
    time::{Duration, Instant},
};

use collection::CollectionRegistry;

use gpui_kit::{Modifiers, TestAppContext};
use request::Method;
use smol::io::{AsyncReadExt, AsyncWriteExt};

use crate::main_view::Page;
use crate::tests::{click, init, no_environments, workspace};
use tab_ui::RequestDraft;

#[gpui_kit::test]
async fn saved_request_opens_with_all_fields_sends_and_keeps_its_tab_state(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/items?from=url", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(
            head.starts_with("POST /items?from=url&tag=edited&tag=two HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("x-saved: updated\r\n"), "{head}");
        assert!(head.contains("x-saved: second\r\n"), "{head}");
        let mut body = [0; 14];
        stream.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"{\"hello\":true}");
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await.unwrap();
    });

    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("Saved API/Items/create.toml");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let original = format!(
        "id = \"create-item\"\nname = \"Create item\"\nschema_version = 1\n[request]\ntype = \"http\"\nmethod = \"POST\"\npath = \"{url}\"\nheaders = [[\"X-Saved\", \"first\"], [\"X-Saved\", \"second\"]]\nquery = [[\"tag\", \"one\"], [\"tag\", \"two\"]]\nbody = {:?}\n",
        b"{\"hello\":true}".as_slice(),
    );
    fs::write(&file, &original).unwrap();
    let collections = CollectionRegistry::from_path(directory.path()).unwrap();

    init(cx);
    let (layout, cx) = workspace(collections, no_environments(), cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, _| window.refresh());
    let row = cx.debug_bounds("collection-row-2").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    let draft = cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].title, "Create item");
        assert_eq!(tabs.tabs[1].method, Some("POST"));
        assert_eq!(
            tabs.tabs[1]
                .location(cx)
                .map(|location| location.path)
                .as_ref(),
            Some(&file)
        );
        let draft = tabs.tabs[1].draft();
        let data = draft.read(cx);
        let location = data.location.as_ref().unwrap();
        assert_eq!(location.name, "Create item");
        assert_eq!(location.collection, "Saved API");
        assert_eq!(data.request.path, url);
        assert_eq!(data.url_input().unwrap().read(cx).value(), url);
        assert_eq!(data.request.headers.len(), 2);
        assert_eq!(data.request.query.len(), 2);
        assert_eq!(
            data.request.body.as_deref(),
            Some(b"{\"hello\":true}".as_slice())
        );
        draft
    });

    // Editing one preloaded row must keep the other saved duplicate intact.
    cx.update(|window, _| window.refresh());
    assert!(
        cx.debug_bounds("headers-key-2").is_some(),
        "keep a trailing blank row"
    );
    let header = cx.debug_bounds("headers-value-0").unwrap();
    cx.simulate_click(header.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("updated");
    let params = cx.debug_bounds("request-section-Params").unwrap();
    cx.simulate_click(params.center(), Modifiers::default());
    let param = cx.debug_bounds("params-value-0").unwrap();
    cx.simulate_click(param.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("edited");
    let body = cx.debug_bounds("request-section-Body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.read(|cx| {
        let data = draft.read(cx);
        assert_eq!(data.body_text_for_test(cx).unwrap(), "{\"hello\":true}");
        assert_eq!(
            data.request.headers,
            [
                ("X-Saved".into(), "updated".into()),
                ("X-Saved".into(), "second".into())
            ]
        );
        assert_eq!(
            data.request.query,
            &[
                ("tag".into(), "edited".into()),
                ("tag".into(), "two".into())
            ]
        );
    });

    cx.simulate_keystrokes("secondary-enter");
    let started = Instant::now();
    while cx.read(|cx| draft.read(cx).is_sending()) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "saved request did not finish"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    server.await;
    assert!(cx.debug_bounds("response-status").is_some());

    cx.update(|_, cx| draft.update(cx, |draft, cx| draft.set_method(Method::Put, cx)));
    cx.simulate_keystrokes("secondary-t");
    cx.simulate_click(row.center(), Modifiers::default());
    cx.simulate_click(row.center(), Modifiers::default());
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].draft(), draft);
        assert_eq!(tabs.tabs[1].method, Some("PUT"));
        assert_eq!(draft.read(cx).request.headers[0].1, "updated");
    });
    assert!(
        cx.debug_bounds("response-status").is_some(),
        "keep the completed response"
    );

    // Save while a text field is focused, then reopen through the registry cache.
    let url_field = cx.debug_bounds("request-url").unwrap();
    cx.simulate_click(url_field.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(&format!("{url}&saved=true"));
    cx.simulate_keystrokes("secondary-s");
    cx.read(|cx| assert!(!draft.read(cx).is_dirty()));
    assert!(cx.debug_bounds("tab-dirty-2").is_none());

    cx.simulate_keystrokes("secondary-w");
    cx.simulate_click(row.center(), Modifiers::default());
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        let reopened = tabs.tabs[2].draft();
        assert_ne!(reopened, draft);
        assert_eq!(reopened.read(cx).request.method, Method::Put);
        assert_eq!(reopened.read(cx).request.headers[0].1, "updated");
        assert_eq!(reopened.read(cx).request.query[0].1, "edited");
        assert_eq!(reopened.read(cx).request.path, format!("{url}&saved=true"));
        assert_eq!(reopened.read(cx).request, draft.read(cx).request);
        assert!(!reopened.read(cx).is_dirty());
    });
    let persisted = collection::FileEntry::from_path(&file).unwrap();
    let request::Request::Http(persisted) = persisted.request else {
        panic!("expected an HTTP request");
    };
    cx.read(|cx| assert_eq!(persisted, draft.read(cx).request));
    assert_ne!(fs::read_to_string(&file).unwrap(), original);
}

pub(crate) struct SavedRequestFixture {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) file: std::path::PathBuf,
    original: String,
}

impl SavedRequestFixture {
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("API/example.toml");
        let original = r#"id = "example"
name = "Example"
schema_version = 1
[request]
type = "http"
method = "POST"
path = "https://example.com/original"
"#
        .to_string();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, &original).unwrap();

        Self {
            directory,
            file,
            original,
        }
    }

    pub(crate) fn open<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (
        gpui_kit::Entity<crate::main_view::MainView>,
        gpui_kit::Entity<RequestDraft>,
        &'a mut gpui_kit::VisualTestContext,
    ) {
        init(cx);
        let registry = CollectionRegistry::from_path(self.directory.path()).unwrap();
        let (layout, cx) = workspace(registry, no_environments(), cx);
        let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
        click(cx, "collection-row-1");
        let draft = cx.read(|cx| tabs.read(cx).tabs[1].draft());

        (tabs, draft, cx)
    }
}

pub(crate) fn edit_url(cx: &mut gpui_kit::VisualTestContext, url: &str) {
    click(cx, "request-url");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(url);
}

#[gpui_kit::test]
fn dirty_close_requires_an_explicit_discard_and_cancel_keeps_edits(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let (tabs, draft, cx) = fixture.open(cx);
    edit_url(cx, "https://example.com/edited");
    assert!(cx.debug_bounds("tab-dirty-2").is_some());

    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));
    click(cx, "cancel-close-request");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_none());
    cx.read(|cx| {
        assert!(draft.read(cx).is_dirty());
        assert_eq!(draft.read(cx).request.path, "https://example.com/edited");
    });

    let tab = cx.debug_bounds("page-tab-2").unwrap();
    cx.simulate_mouse_move(tab.center(), None, Modifiers::default());
    click(cx, "close-tab-2");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    click(cx, "discard-request-changes");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
    assert_eq!(fs::read_to_string(&fixture.file).unwrap(), fixture.original);
    click(cx, "collection-row-1");
    cx.read(|cx| {
        let reopened = tabs.read(cx).tabs[1].draft();
        assert_eq!(
            reopened.read(cx).request.path,
            "https://example.com/original"
        );
        assert!(!reopened.read(cx).is_dirty());
    });
}

#[gpui_kit::test]
fn save_and_close_persists_body_and_does_not_close_after_a_failed_save(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let (tabs, draft, cx) = fixture.open(cx);
    click(cx, "request-section-Body");
    click(cx, "request-body");
    cx.simulate_input("{\"persisted\":true}");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"{\"persisted\":true}".as_slice())
        )
    });
    fs::remove_file(&fixture.file).unwrap();

    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("request-save-error").is_some());
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));
    cx.simulate_keystrokes("secondary-w");
    click(cx, "save-and-close-request");
    assert!(cx.debug_bounds("request-save-error").is_some());
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));

    fs::write(&fixture.file, &fixture.original).unwrap();
    click(cx, "save-and-close-request");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
    click(cx, "collection-row-1");
    cx.read(|cx| {
        let reopened = tabs.read(cx).tabs[1].draft();
        assert_eq!(reopened.read(cx).request.body, draft.read(cx).request.body);
        assert!(!reopened.read(cx).is_dirty());
    });
    let request::Request::Http(saved) = collection::FileEntry::from_path(&fixture.file)
        .unwrap()
        .request
    else {
        panic!("expected an HTTP request");
    };
    assert_eq!(
        saved.body.as_deref(),
        Some(b"{\"persisted\":true}".as_slice())
    );
}

#[gpui_kit::test]
fn reverting_an_edit_clears_dirty_state_and_closes_without_prompt(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let (tabs, draft, cx) = fixture.open(cx);
    edit_url(cx, "https://example.com/edited");
    edit_url(cx, "https://example.com/original");
    cx.read(|cx| assert!(!draft.read(cx).is_dirty()));
    assert!(cx.debug_bounds("tab-dirty-2").is_none());
    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
    assert!(cx.debug_bounds("unsaved-request-prompt").is_none());
}

#[gpui_kit::test]
fn enter_opens_the_selected_request_and_f2_renames_it(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    init(cx);
    let registry = CollectionRegistry::from_path(fixture.directory.path()).unwrap();
    let (layout, cx) = workspace(registry, no_environments(), cx);
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();

    // Enter opens a collection's own page.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.tabs[1].title, "API");
        assert!(matches!(tabs.tabs[1].page, Page::Collection(_)));
    });
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());

    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        assert_eq!(tabs.selected, Some(2));
        assert_eq!(
            tabs.tabs[2]
                .location(cx)
                .map(|location| location.path)
                .as_ref(),
            Some(&fixture.file)
        );
    });
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());

    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.simulate_input("Renamed");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        assert_eq!(tabs.tabs[2].title, "Renamed");
    });
}

#[gpui_kit::test]
fn saving_follows_collection_renames_and_request_moves(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    fs::create_dir_all(fixture.directory.path().join("Other")).unwrap();
    let (tabs, draft, cx) = fixture.open(cx);
    edit_url(cx, "https://example.com/edited-before-rename");

    // Select the collection without opening its tab, so Save targets the request.
    click(cx, "collection-row-1");
    cx.simulate_keystrokes("up f2");
    cx.simulate_input("Renamed API");
    cx.simulate_keystrokes("enter");
    let renamed = fixture.directory.path().join("Renamed API/example.toml");
    cx.read(|cx| {
        assert_eq!(
            tabs.read(cx).tabs[1]
                .location(cx)
                .map(|location| location.path)
                .as_ref(),
            Some(&renamed)
        );
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().collection,
            "Renamed API"
        );
        assert!(draft.read(cx).is_dirty());
    });
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("request-save-error").is_none());
    let request::Request::Http(saved) = collection::FileEntry::from_path(&renamed).unwrap().request
    else {
        panic!("expected an HTTP request");
    };
    assert_eq!(saved.path, "https://example.com/edited-before-rename");

    // Drag the open request to another collection.
    let source = cx.debug_bounds("collection-row-1").unwrap().center();
    let target = cx.debug_bounds("collection-row-2").unwrap().center();
    cx.simulate_event(gpui_kit::MouseDownEvent {
        button: gpui_kit::MouseButton::Left,
        position: source,
        click_count: 1,
        ..Default::default()
    });
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: target,
        pressed_button: Some(gpui_kit::MouseButton::Left),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.simulate_event(gpui_kit::MouseUpEvent {
        button: gpui_kit::MouseButton::Left,
        position: target,
        click_count: 1,
        ..Default::default()
    });
    cx.run_until_parked();
    let moved = fixture.directory.path().join("Other/example.toml");
    cx.read(|cx| {
        assert_eq!(
            tabs.read(cx).tabs[1]
                .location(cx)
                .map(|location| location.path)
                .as_ref(),
            Some(&moved)
        );
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().collection,
            "Other"
        );
    });
    edit_url(cx, "https://example.com/edited-after-move");
    cx.simulate_keystrokes("secondary-s");
    cx.simulate_keystrokes("secondary-w");
    click(cx, "collection-row-2");
    cx.read(|cx| {
        assert_eq!(tabs.read(cx).tabs.len(), 2);
        let reopened = tabs.read(cx).tabs[1].draft();
        assert_eq!(
            reopened.read(cx).request.path,
            "https://example.com/edited-after-move"
        );
        assert!(!reopened.read(cx).is_dirty());
    });
}

fn context_click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let position = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(gpui_kit::MouseDownEvent {
        button: gpui_kit::MouseButton::Right,
        position,
        click_count: 1,
        ..Default::default()
    });
    cx.simulate_event(gpui_kit::MouseUpEvent {
        button: gpui_kit::MouseButton::Right,
        position,
        click_count: 1,
        ..Default::default()
    });
}

#[gpui_kit::test]
fn deleted_request_cannot_save_over_a_new_request_at_the_same_path(cx: &mut TestAppContext) {
    let mut fixture = SavedRequestFixture::new();
    let reused = fixture.directory.path().join("API/New Request.toml");
    fs::rename(&fixture.file, &reused).unwrap();
    fixture.file = reused;
    let (tabs, draft, cx) = fixture.open(cx);
    edit_url(cx, "https://example.com/old-draft");

    click(cx, "collection-row-1");
    cx.simulate_keystrokes("backspace");
    click(cx, "confirm-sidebar-delete");
    assert!(!fixture.file.exists());

    context_click(cx, "collection-row-0");
    cx.simulate_keystrokes("down enter");
    cx.simulate_input("Different request");
    cx.simulate_keystrokes("enter");
    let before = collection::FileEntry::from_path(&fixture.file).unwrap();
    assert_ne!(before.id, "example");
    assert_eq!(before.name, "Different request");

    click(cx, "page-tab-2");
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("request-save-error").is_some());
    let after = collection::FileEntry::from_path(&fixture.file).unwrap();
    assert_eq!(after.id, before.id);
    let request::Request::Http(after_request) = after.request else {
        panic!("expected an HTTP request");
    };
    assert_eq!(after_request.path, "/", "old tab overwrote a new request");
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));

    click(cx, "collection-row-1");
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3, "a new file needs its own draft");
        assert_eq!(
            tabs.tabs[2]
                .location(cx)
                .map(|location| location.id)
                .as_deref(),
            Some(before.id.as_str())
        );
        assert_ne!(tabs.tabs[2].draft(), draft);
    });
    edit_url(cx, "https://example.com/new-draft");
    cx.simulate_keystrokes("secondary-s");
    let request::Request::Http(saved) = collection::FileEntry::from_path(&fixture.file)
        .unwrap()
        .request
    else {
        panic!("expected an HTTP request");
    };
    assert_eq!(saved.path, "https://example.com/new-draft");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "https://example.com/old-draft"));
}

#[gpui_kit::test]
fn second_close_shortcut_discards_only_the_pending_tab(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let (tabs, _, cx) = fixture.open(cx);
    edit_url(cx, "https://example.com/discard-me");
    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));

    // Cancelling starts a fresh confirmation sequence.
    click(cx, "cancel-close-request");
    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));
    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_none());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
    assert_eq!(fs::read_to_string(&fixture.file).unwrap(), fixture.original);

    // The next dirty tab must get its own first-press confirmation.
    edit_url(cx, "https://example.com/other-draft");
    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 1));
    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert!(tabs.read(cx).tabs.is_empty()));
}

#[gpui_kit::test]
fn breadcrumbs_include_nested_folders_and_follow_folder_renames(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let nested = fixture.directory.path().join("API/API/V2/example.toml");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::rename(&fixture.file, &nested).unwrap();
    init(cx);
    let registry = CollectionRegistry::from_path(fixture.directory.path()).unwrap();
    let (layout, cx) = workspace(registry, no_environments(), cx);
    click(cx, "collection-row-3");
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    let draft = cx.read(|cx| tabs.read(cx).tabs[1].draft());
    cx.read(|cx| {
        assert_eq!(draft.read(cx).location.as_ref().unwrap().collection, "API");
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().folders,
            vec![gpui_kit::SharedString::from("API"), "V2".into()]
        );
    });
    cx.update(|window, _| window.refresh());
    let collection = cx.debug_bounds("request-collection").unwrap();
    let folder = cx.debug_bounds("request-folder-0").unwrap();
    let nested_folder = cx.debug_bounds("request-folder-1").unwrap();
    let name = cx.debug_bounds("request-name").unwrap();
    assert!(collection.right() < folder.left());
    assert!(folder.right() < nested_folder.left());
    assert!(nested_folder.right() < name.left());

    let method = cx.debug_bounds("request-method").unwrap();
    let label = cx.debug_bounds("request-method-label").unwrap();
    let arrow = cx.debug_bounds("request-method-arrow").unwrap();
    assert!(label.left() < method.left() + gpui_kit::px(20.));
    assert!(arrow.right() > method.right() - gpui_kit::px(20.));

    click(cx, "collection-row-1");
    cx.simulate_keystrokes("f2");
    cx.simulate_input("Renamed folder");
    cx.simulate_keystrokes("enter");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().folders,
            vec![gpui_kit::SharedString::from("Renamed folder"), "V2".into()]
        );
        assert_eq!(draft.read(cx).location.as_ref().unwrap().name, "Example");
    });
}
