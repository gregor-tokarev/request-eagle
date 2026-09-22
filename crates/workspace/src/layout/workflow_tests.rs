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
