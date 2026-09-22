use collection::CollectionRegistry;
use gpui_kit::{Modifiers, TestAppContext};

use super::request_draft::RequestDraft;

#[gpui_kit::test]
fn postman_import_creates_saved_requests_and_keeps_folders(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let collections = CollectionRegistry::from_path(directory.path()).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let (layout, cx) = cx.add_window_view(|window, cx| {
        crate::workspace::Layout::new(collections, updater::init("1.2.3", cx), window, cx)
    });
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    let source = r#"{"info":{"name":"Imported API","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"Accounts","item":[{"name":"Read account","request":{"method":"GET","url":"https://example.com/account","auth":{"type":"bearer","bearer":[{"key":"token","value":"{{token}}"}]}}}]}]}"#;
    cx.update(|window, cx| view.update(cx, |view, cx| view.import_text(source, window, cx)));
    let path = cx.read(|cx| {
        let view = view.read(cx);
        assert!(view.workflow_error.is_none(), "{:?}", view.workflow_error);
        assert_eq!(view.tabs.len(), 2);
        let tab = &view.tabs[1];
        let path = tab.request_path.clone().expect("saved import");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "Accounts");
        let draft = tab.page.clone().downcast::<RequestDraft>().ok().unwrap();
        assert_eq!(draft.read(cx).request.path, "https://example.com/account");
        assert_eq!(
            draft.read(cx).saved_request.path,
            "https://example.com/account"
        );
        assert_eq!(
            draft.read(cx).environment_path.as_deref(),
            Some(
                path.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("environment.toml")
                    .as_path()
            )
        );
        path
    });
    let reloaded = CollectionRegistry::from_path(directory.path()).unwrap();
    let saved = reloaded.file(&path).unwrap();
    assert_eq!(saved.name, "Read account");
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[1].request_id.as_deref(),
            Some(saved.id.as_str())
        )
    });
    let collection::Request::Http(request) = &saved.request;
    assert!(
        matches!(&request.authentication, request::Authentication::Bearer {token} if token == "{{token}}")
    );

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.import_text("curl --unsupported https://example.com", window, cx)
        })
    });
    cx.read(|cx| {
        assert!(
            view.read(cx)
                .workflow_error
                .as_ref()
                .unwrap()
                .contains("Unsupported cURL option")
        );
        assert_eq!(view.read(cx).tabs.len(), 2);
    });
    assert_eq!(
        CollectionRegistry::from_path(directory.path())
            .unwrap()
            .len(),
        1
    );
}

#[gpui_kit::test]
fn history_reopens_an_editable_copy_and_clears_persisted_entries(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut view = super::main_view::MainView::new(cx);
        view.enable_workflow_storage(directory.path().to_path_buf(), cx);
        view
    });
    let draft = cx.read(|cx| {
        view.read(cx).tabs[0]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.update(|_, cx| {
        draft.update(cx, |_, cx| {
            cx.emit(crate::history::HistoryEntry::new(
                "Account".into(),
                request::HttpRequest {
                    path: "https://{{host}}/account".into(),
                    ..Default::default()
                },
                Some(directory.path().join("environment.toml")),
            ));
        })
    });
    let history = cx.debug_bounds("request-history").unwrap();
    cx.simulate_click(history.center(), Modifiers::default());
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(view.history_visible);
        assert_eq!(
            view.history.entries[0].request.path,
            "https://{{host}}/account"
        );
    });
    let entry = cx.debug_bounds("history-entry-0").unwrap();
    cx.simulate_click(entry.center(), Modifiers::default());
    cx.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.tabs.len(), 2);
        assert!(view.tabs[1].request_path.is_none());
        let copy = view.tabs[1]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        assert_eq!(copy.read(cx).request.path, "https://{{host}}/account");
        assert_eq!(
            copy.read(cx).environment_path,
            Some(directory.path().join("environment.toml"))
        );
        assert!(draft.read(cx).request.path.is_empty());
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.history.clear().unwrap();
            cx.notify();
        })
    });
    assert!(
        crate::history::History::load(directory.path().join("history.json"))
            .unwrap()
            .entries
            .is_empty()
    );
}

#[gpui_kit::test]
fn recovered_saved_drafts_keep_dirty_state_and_save_auth_and_forms(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let collections_path = directory.path().join("collections");
    let state = directory.path().join("state");
    let mut collections = CollectionRegistry::from_path(&collections_path).unwrap();
    let original = request::HttpRequest {
        path: "https://example.com/original".into(),
        ..Default::default()
    };
    let imported = collections
        .import_requests(
            "API",
            vec![collection::ImportedRequest {
                name: "Account".into(),
                folders: vec![],
                request: original.clone(),
            }],
        )
        .unwrap();
    let file = &imported[0];
    let edited = request::HttpRequest {
        method: request::Method::Patch,
        path: "https://example.com/edited".into(),
        authentication: request::Authentication::Bearer {
            token: "{{token}}".into(),
        },
        form: Some(request::FormBody::UrlEncoded(vec![(
            "name".into(),
            "first & last".into(),
        )])),
        ..Default::default()
    };
    crate::session::SessionStore::new(state.join("session.json"))
        .save(&crate::session::SessionSnapshot {
            selected: Some(0),
            tabs: vec![crate::session::RecoveredTab {
                title: "Account".into(),
                name: "Account".into(),
                collection: Some("API".into()),
                request_id: Some(file.id.clone()),
                request_path: Some(file.path.clone()),
                environment_path: Some(file.collection_path.join("environment.toml")),
                request: edited.clone(),
                saved_request: Some(original),
            }],
        })
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let (layout, cx) = cx.add_window_view(|window, cx| {
        crate::workspace::Layout::new(collections, updater::init("1.2.3", cx), window, cx)
    });
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.enable_workflow_storage(state.clone(), cx);
            view.focus(window, cx);
        })
    });
    let draft = cx.read(|cx| {
        view.read(cx).tabs[0]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));
    cx.simulate_keystrokes("secondary-w");
    assert!(cx.debug_bounds("unsaved-request-prompt").is_some());
    let cancel = cx.debug_bounds("cancel-close-request").unwrap();
    cx.simulate_click(cancel.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-s");
    cx.read(|cx| assert!(!draft.read(cx).is_dirty()));
    let reloaded = CollectionRegistry::from_path(&collections_path).unwrap();
    let collection::Request::Http(saved) = &reloaded.file(&file.path).unwrap().request;
    assert_eq!(saved, &edited);
    cx.update(|_, cx| view.update(cx, |view, cx| view.flush_session(cx)));
    let snapshot = crate::session::SessionStore::new(state.join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.tabs[0].saved_request.as_ref(), Some(&edited));
    assert_eq!(snapshot.tabs[0].request, edited);
    cx.simulate_keystrokes("secondary-w");
    cx.read(|cx| assert!(view.read(cx).tabs.is_empty()));
}

#[gpui_kit::test]
async fn sidebar_relocation_updates_environment_recovery_and_execution(cx: &mut TestAppContext) {
    use std::{fs, time::Duration};

    use smol::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
        cx.update(|window, _| window.refresh());
        let bounds = cx.debug_bounds(selector).unwrap();
        cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let collections_path = directory.path().join("collections");
    let state = directory.path().join("state");
    let source = collections_path.join("API");
    let destination = collections_path.join("Other");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        source.join("example.toml"),
        r#"id = "example"
name = "Example"
schema_version = 1
[request]
type = "http"
method = "GET"
path = "{{base_url}}/{{account}}"
headers = [["X-Collection", "{{account}}"]]
[request.authentication]
type = "bearer"
token = "{{token}}"
"#,
    )
    .unwrap();

    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    for (root, account) in [(&source, "original"), (&destination, "destination")] {
        fs::write(
            root.join("environment.toml"),
            format!(
                "base_url = \"{url}\"\naccount = \"{account}\"\ntoken = \"{account}-secret\"\n"
            ),
        )
        .unwrap();
    }
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();

        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }

        let head = String::from_utf8(head).unwrap();
        assert!(head.starts_with("GET /destination HTTP/1.1\r\n"), "{head}");
        assert!(head.contains("x-collection: destination\r\n"), "{head}");
        assert!(
            head.contains("authorization: Bearer destination-secret\r\n"),
            "{head}"
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });

    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let collections = CollectionRegistry::from_path(&collections_path).unwrap();
    let (layout, cx) = cx.add_window_view(|window, cx| {
        crate::workspace::Layout::new(collections, updater::init("1.2.3", cx), window, cx)
    });
    let view = cx.read(|cx| layout.read(cx).main_view.clone());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.enable_workflow_storage(state.clone(), cx)
        });
    });
    click(cx, "collection-row-1");
    let draft = cx.read(|cx| {
        view.read(cx).tabs[1]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).environment_path,
            Some(source.join("environment.toml"))
        )
    });

    click(cx, "collection-row-0");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Renamed API");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let renamed = collections_path.join("Renamed API");
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[1].request_path,
            Some(renamed.join("example.toml"))
        );
        assert_eq!(
            draft.read(cx).environment_path,
            Some(renamed.join("environment.toml"))
        );
    });
    let snapshot = crate::session::SessionStore::new(state.join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.tabs[1].request_path,
        Some(renamed.join("example.toml"))
    );
    assert_eq!(
        snapshot.tabs[1].environment_path,
        Some(renamed.join("environment.toml"))
    );

    click(cx, "collection-row-0");
    let from = cx.debug_bounds("collection-row-1").unwrap().center();
    let to = cx.debug_bounds("collection-row-2").unwrap().center();
    cx.simulate_event(gpui_kit::MouseDownEvent {
        button: gpui_kit::MouseButton::Left,
        position: from,
        click_count: 1,
        ..Default::default()
    });
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: to,
        pressed_button: Some(gpui_kit::MouseButton::Left),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.simulate_event(gpui_kit::MouseUpEvent {
        button: gpui_kit::MouseButton::Left,
        position: to,
        click_count: 1,
        ..Default::default()
    });
    cx.run_until_parked();

    cx.read(|cx| {
        assert_eq!(
            view.read(cx).tabs[1].request_path,
            Some(destination.join("example.toml"))
        );
        assert_eq!(
            draft.read(cx).environment_path,
            Some(destination.join("environment.toml"))
        );
        assert_eq!(draft.read(cx).request.path, "{{base_url}}/{{account}}");
    });
    let snapshot = crate::session::SessionStore::new(state.join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.tabs[1].request_path,
        Some(destination.join("example.toml"))
    );
    assert_eq!(
        snapshot.tabs[1].environment_path,
        Some(destination.join("environment.toml"))
    );

    click(cx, "send-request");
    smol::future::or(server, async {
        smol::Timer::after(Duration::from_secs(5)).await;
        panic!("relocated request did not reach the destination environment endpoint");
    })
    .await;
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).history.entries[0].environment_path,
            Some(destination.join("environment.toml"))
        );
    });
}
