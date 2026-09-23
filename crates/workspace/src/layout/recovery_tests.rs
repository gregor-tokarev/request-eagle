use std::{fs, path::Path};

use collection::{HttpRequest, Method};
use gpui_kit::{AppContext, Entity, TestAppContext};

use super::{main_view::MainView, request_draft::RequestDraft};
use crate::session::SessionStore;

#[test]
fn collection_state_is_outside_the_registry_and_distinct_per_fixture() {
    let first = super::recovery::collection_state_directory(Path::new("/fixtures/first")).unwrap();
    let second =
        super::recovery::collection_state_directory(Path::new("/fixtures/second")).unwrap();

    assert_eq!(first, Path::new("/fixtures/.first.request-eagle-state"));
    assert_eq!(second, Path::new("/fixtures/.second.request-eagle-state"));
    assert!(!first.starts_with("/fixtures/first"));
    assert_ne!(first, second);
    assert_eq!(
        super::recovery::collection_state_directory(Path::new("fixtures/first/")),
        Some(Path::new("fixtures/.first.request-eagle-state").to_path_buf())
    );
}

#[test]
fn collection_state_resolves_current_and_parent_directory_roots() {
    let current = std::env::current_dir().unwrap().canonicalize().unwrap();
    let parent = current.parent().unwrap();

    for (relative, absolute) in [
        (Path::new("."), current.as_path()),
        (Path::new(".."), parent),
    ] {
        let state = super::recovery::collection_state_directory(relative).unwrap();

        assert_eq!(
            Some(&state),
            super::recovery::collection_state_directory(absolute).as_ref()
        );
        assert!(!state.starts_with(absolute));
    }
}

#[gpui_kit::test]
fn parent_component_collection_roots_keep_session_recovery(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let collections = fixture.path().join("collections");
    let child = collections.join("child");
    fs::create_dir_all(&child).unwrap();
    let relative_root = child.join("..");
    let state = super::recovery::collection_state_directory(&relative_root).unwrap();
    let view = restored_view(&state, cx);
    let draft = draft_at(&view, 0, cx);

    draft.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/recovered-relative-root".into();
        cx.notify();
    });
    cx.run_until_parked();
    drop(draft);
    drop(view);
    cx.run_until_parked();

    let named_state =
        super::recovery::collection_state_directory(&collections.canonicalize().unwrap()).unwrap();
    assert_eq!(state, named_state);
    let restored = restored_view(&named_state, cx);
    let restored_draft = draft_at(&restored, 0, cx);

    cx.read(|cx| {
        assert_eq!(
            restored_draft.read(cx).request.path,
            "https://example.test/recovered-relative-root"
        );
    });
}

#[gpui_kit::test]
fn recovered_requests_keep_identity_when_relative_root_becomes_absolute(cx: &mut TestAppContext) {
    assert_recovered_root_identity("collections", cx);
}

#[gpui_kit::test]
fn recovered_requests_keep_identity_when_parent_components_are_removed(cx: &mut TestAppContext) {
    assert_recovered_root_identity("child/../collections", cx);
}

fn assert_recovered_root_identity(root: &str, cx: &mut TestAppContext) {
    use std::{cell::RefCell, rc::Rc};

    use collection::{CollectionRegistry, FileEntry};

    use super::main_view::RequestSaveRequested;

    let fixture = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(fixture.path().join("child")).unwrap();
    let relative_root = Path::new(fixture.path().file_name().unwrap()).join(root);
    assert!(relative_root.is_relative());
    let mut registry = CollectionRegistry::from_path(&relative_root).unwrap();
    let collection = registry.create_collection().unwrap();
    let path = registry.create_request(&collection).unwrap();
    let file = FileEntry::from_path(&path).unwrap();
    let environment = collection.join("environment.toml");
    fs::write(&environment, "base_url = 'https://example.test'\n").unwrap();
    let state = super::recovery::collection_state_directory(&relative_root).unwrap();
    let view = restored_view(&state, cx);

    view.update(cx, |view, cx| {
        view.close_tab(0, cx);
        view.collection_paths = vec![collection.clone()];
        view.open_request(
            &path,
            file.id.clone().into(),
            file.name.clone().into(),
            "API".into(),
            &file.request,
            cx,
        );
    });
    let draft = draft_at(&view, 0, cx);
    draft.update(cx, |draft, cx| {
        draft.request.path = "{{base_url}}/edited".into();
        cx.notify();
    });
    let attempt = cx.read(|cx| {
        let draft = draft.read(cx);
        crate::history::HistoryEntry::new(
            draft.name.to_string(),
            draft.request.clone(),
            draft.environment_path.clone(),
        )
    });
    view.update(cx, |view, cx| view.record_history(attempt, cx));
    cx.run_until_parked();
    drop(draft);
    drop(view);
    cx.run_until_parked();

    let absolute_root = std::path::absolute(fixture.path().join("collections")).unwrap();
    let absolute_state = super::recovery::collection_state_directory(&absolute_root).unwrap();
    assert_eq!(
        state.canonicalize().unwrap(),
        absolute_state.canonicalize().unwrap()
    );
    let registry = Rc::new(RefCell::new(
        CollectionRegistry::from_path(&absolute_root).unwrap(),
    ));
    let absolute_collection = registry.borrow().collections()[0].path.clone();
    let absolute_file =
        FileEntry::from_path(&absolute_collection.join(path.file_name().unwrap())).unwrap();
    let restored = restored_view(&absolute_state, cx);
    restored.update(cx, |view, cx| {
        view.collection_paths = vec![absolute_collection];
        view.open_request(
            &absolute_file.path,
            absolute_file.id.clone().into(),
            absolute_file.name.clone().into(),
            "API".into(),
            &absolute_file.request,
            cx,
        );
    });
    let restored_draft = draft_at(&restored, 0, cx);
    cx.read(|cx| {
        let view = restored.read(cx);
        let draft = restored_draft.read(cx);
        assert_eq!(
            view.tabs.len(),
            1,
            "Sidebar navigation must reuse the recovered tab"
        );
        assert_eq!(draft.request.path, "{{base_url}}/edited");
        assert!(draft.is_dirty());
        assert!(view.tabs[0].request_path.as_ref().unwrap().is_absolute());
        assert!(draft.environment_path.as_ref().unwrap().is_absolute());
        assert_eq!(
            view.history.entries[0].environment_path,
            draft.environment_path
        );
        let environment =
            environment::Environment::from_file(draft.environment_path.as_ref().unwrap()).unwrap();
        let resolved =
            request::resolve_variables(&view.history.entries[0].request, &environment.entries)
                .unwrap();
        assert_eq!(resolved.path, "https://example.test/edited");
    });

    let results = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let registry = registry.clone();
        let results = results.clone();
        cx.subscribe(&restored, move |_, event: &RequestSaveRequested, _| {
            results
                .borrow_mut()
                .push(registry.borrow_mut().update_request(
                    &event.path,
                    &event.request_id,
                    event.request.clone().into(),
                ));
        })
    });
    restored.update(cx, |view, cx| view.save_active_request(cx));
    cx.run_until_parked();
    assert_eq!(results.borrow().len(), 1);
    assert!(results.borrow()[0].is_ok(), "{:?}", results.borrow()[0]);
    let collection::Request::Http(saved) =
        FileEntry::from_path(&absolute_file.path).unwrap().request;
    assert_eq!(saved.path, "{{base_url}}/edited");
    drop(subscription);
    drop(restored_draft);
    drop(restored);
    cx.run_until_parked();
}

#[cfg(unix)]
#[test]
fn collection_state_keeps_non_utf8_fixture_names() {
    use std::os::unix::ffi::OsStrExt as _;

    let path = Path::new(std::ffi::OsStr::from_bytes(b"/fixtures/api-\xff"));
    let state = super::recovery::collection_state_directory(path).unwrap();

    assert_eq!(
        state.as_os_str().as_bytes(),
        b"/fixtures/.api-\xff.request-eagle-state"
    );
}

fn restored_view(directory: &Path, cx: &mut TestAppContext) -> Entity<MainView> {
    let view = cx.new(MainView::new);
    view.update(cx, |view, cx| {
        view.enable_workflow_storage(directory.to_path_buf(), cx);
    });

    view
}

fn draft_at(view: &Entity<MainView>, index: usize, cx: &TestAppContext) -> Entity<RequestDraft> {
    cx.read(|cx| {
        view.read(cx).tabs[index]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    })
}

#[gpui_kit::test]
fn recovery_observes_edits_and_restores_saved_and_untitled_requests(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let collection_path = directory.path().join("API");
    let saved_path = collection_path.join("request.toml");
    let environment_path = collection_path.join("environment.toml");
    let view = restored_view(directory.path(), cx);
    let untitled = draft_at(&view, 0, cx);

    untitled.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/unsaved".into();
        draft.request.headers = vec![("X-Draft".into(), "untitled".into())];
        draft.request.body = Some(br#"{"draft":true}"#.to_vec());
        draft.request.authentication = request::Authentication::Bearer {
            token: "{{token}}".into(),
        };
        draft.environment_path = Some(environment_path.clone());
        cx.notify();
    });
    cx.run_until_parked();

    // The observer must save edits before a tab switch or orderly shutdown.
    let on_disk = SessionStore::new(directory.path().join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(on_disk.tabs[0].request.path, "https://example.test/unsaved");
    assert!(
        on_disk.tabs[0]
            .saved_request
            .as_ref()
            .unwrap()
            .path
            .is_empty()
    );

    let baseline = HttpRequest {
        method: Method::Post,
        path: "https://example.test/original".into(),
        query: Some(vec![("original".into(), "value".into())]),
        ..Default::default()
    };
    view.update(cx, |view, cx| {
        view.collection_paths = vec![collection_path.clone()];
        view.open_request(
            &saved_path,
            "original-request-id".into(),
            "Create item".into(),
            "API".into(),
            &baseline.clone().into(),
            cx,
        );
    });
    let saved = draft_at(&view, 1, cx);
    saved.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/edited".into();
        draft.request.headers = vec![("Authorization".into(), "Bearer edited".into())];
        draft.request.query = Some(vec![("edited".into(), "value".into())]);
        draft.request.body = Some(vec![0, 1, 255]);
        draft.set_method(Method::Put, cx);
        cx.notify();
    });
    cx.run_until_parked();

    view.update(cx, |view, cx| {
        view.new_tab(cx);
        view.select_tab(0, cx);
        view.close_tab(2, cx);
    });
    cx.run_until_parked();
    let expected = cx.read(|cx| view.read(cx).session_snapshot(cx));

    drop(saved);
    drop(untitled);
    drop(view);
    cx.run_until_parked();

    let restored = restored_view(directory.path(), cx);
    cx.read(|cx| {
        let restored = restored.read(cx);
        let actual = restored.session_snapshot(cx);

        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.selected, Some(0));
        assert_eq!(restored.tabs[1].method, Some("PUT"));
        assert_eq!(restored.tabs[1].request_path.as_ref(), Some(&saved_path));
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    });
    let recovered_saved = draft_at(&restored, 1, cx);
    cx.read(|cx| {
        let draft = recovered_saved.read(cx);

        assert_eq!(draft.environment_path.as_ref(), Some(&environment_path));
        assert_eq!(draft.collection.as_deref(), Some("API"));
        assert_eq!(draft.name.as_ref(), "Create item");
        assert_eq!(draft.saved_request.path, baseline.path);
        assert_eq!(draft.request.path, "https://example.test/edited");
    });

    // Collection navigation must reuse the recovered file identity and edits.
    restored.update(cx, |view, cx| {
        view.open_request(
            &saved_path,
            "original-request-id".into(),
            "Create item".into(),
            "API".into(),
            &baseline.into(),
            cx,
        );
    });
    cx.read(|cx| {
        assert_eq!(restored.read(cx).tabs.len(), 2);
        assert_eq!(restored.read(cx).selected, Some(1));
        assert_eq!(
            recovered_saved.read(cx).request.path,
            "https://example.test/edited"
        );
    });
}

#[gpui_kit::test]
fn recovery_keeps_an_explicitly_empty_workspace_empty(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let view = restored_view(directory.path(), cx);

    view.update(cx, |view, cx| view.close_active_tab(cx));
    cx.run_until_parked();
    drop(view);

    let restored = restored_view(directory.path(), cx);
    cx.read(|cx| {
        assert!(restored.read(cx).tabs.is_empty());
        assert!(restored.read(cx).selected.is_none());
    });

    let snapshot = SessionStore::new(directory.path().join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert!(snapshot.tabs.is_empty());
    assert!(snapshot.selected.is_none());
}

#[gpui_kit::test]
fn recovery_preserves_selection_after_closing_an_earlier_tab(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let view = restored_view(directory.path(), cx);

    view.update(cx, |view, cx| {
        view.new_tab(cx);
        view.new_tab(cx);
        view.close_tab(0, cx);
    });
    cx.run_until_parked();
    let selected_title = cx.read(|cx| {
        let view = view.read(cx);

        assert_eq!(view.selected, Some(1));

        view.tabs[1].title.clone()
    });
    drop(view);

    let restored = restored_view(directory.path(), cx);
    cx.read(|cx| {
        let restored = restored.read(cx);

        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.selected, Some(1));
        assert_eq!(restored.tabs[1].title, selected_title);
    });
}

#[gpui_kit::test]
fn recovery_reports_corrupt_storage_without_overwriting_it(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let corrupt = b"{incomplete recovery data";
    fs::write(&path, corrupt).unwrap();

    let view = restored_view(directory.path(), cx);
    cx.read(|cx| {
        let view = view.read(cx);

        assert!(view.recovery.is_none());
        assert!(
            view.storage_error
                .as_deref()
                .unwrap()
                .contains("Could not recover the previous session")
        );
    });
    let draft = draft_at(&view, 0, cx);
    draft.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/new-work".into();
        cx.notify();
    });
    view.update(cx, |view, cx| {
        view.new_tab(cx);
        view.close_active_tab(cx);
    });
    cx.run_until_parked();

    assert_eq!(fs::read(path).unwrap(), corrupt);
}

#[gpui_kit::test]
fn recovery_ignores_notifications_without_request_changes(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let view = restored_view(directory.path(), cx);
    let draft = draft_at(&view, 0, cx);
    cx.run_until_parked();
    let revision = cx.read(|cx| view.read(cx).recovery_revision());

    for _ in 0..5 {
        draft.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }

    cx.read(|cx| assert_eq!(view.read(cx).recovery_revision(), revision));
}

#[gpui_kit::test]
fn recovery_flushes_final_edit_on_quit_before_observers_or_workers_run(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let view = restored_view(directory.path(), cx);
    let draft = draft_at(&view, 0, cx);

    // Several older checkpoints can still be queued when quitting begins.
    for index in 0..5 {
        draft.update(cx, |draft, cx| {
            draft.request.path = format!("https://example.test/queued/{index}");
            cx.notify();
        });
    }

    cx.update(|cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = "https://example.test/final-edit".into();
            cx.notify();
        });

        cx.shutdown();
    });

    let final_snapshot = SessionStore::new(&path).load().unwrap().unwrap();
    assert_eq!(
        final_snapshot.tabs[0].request.path,
        "https://example.test/final-edit"
    );

    // If older background jobs are resumed, their generations must be skipped.
    cx.run_until_parked();
    let after_workers = SessionStore::new(path).load().unwrap().unwrap();
    assert_eq!(
        after_workers.tabs[0].request.path,
        "https://example.test/final-edit"
    );
}

#[gpui_kit::test]
fn recovery_reports_background_write_failure_and_recovers_on_next_edit(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let view = restored_view(directory.path(), cx);
    let draft = draft_at(&view, 0, cx);
    cx.run_until_parked();

    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    draft.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/blocked".into();
        cx.notify();
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            view.read(cx)
                .storage_error
                .as_deref()
                .unwrap()
                .starts_with("Could not save session recovery")
        );
    });

    fs::remove_dir(&path).unwrap();
    draft.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/retried".into();
        cx.notify();
    });
    cx.run_until_parked();

    cx.read(|cx| assert!(view.read(cx).storage_error.is_none()));
    assert_eq!(
        SessionStore::new(path).load().unwrap().unwrap().tabs[0]
            .request
            .path,
        "https://example.test/retried"
    );
}

#[gpui_kit::test]
fn recovery_keeps_original_identity_when_saved_path_is_reused(cx: &mut TestAppContext) {
    use std::{cell::RefCell, rc::Rc};

    use super::main_view::RequestSaveRequested;
    use crate::session::{RecoveredTab, SessionSnapshot};

    // Legacy snapshots have no identity. They must remain unsavable rather
    // than acquiring the identity of whichever file now occupies the path.
    for captured_id in [Some("original-request-id"), None] {
        let directory = tempfile::tempdir().unwrap();
        let file_path = directory.path().join("request.toml");
        let state = directory.path().join("state");
        fs::write(
            &file_path,
            r#"id = "replacement-request-id"
name = "Replacement"
schema_version = 1
[request]
type = "http"
method = "GET"
path = "https://example.test/replacement"
"#,
        )
        .unwrap();
        SessionStore::new(state.join("session.json"))
            .save(&SessionSnapshot {
                selected: Some(0),
                tabs: vec![RecoveredTab {
                    title: "Original".into(),
                    name: "Original".into(),
                    collection: Some("API".into()),
                    request_path: Some(file_path.clone()),
                    request_id: captured_id.map(Into::into),
                    environment_path: None,
                    request: HttpRequest {
                        path: "https://example.test/original-edited".into(),
                        ..Default::default()
                    },
                    saved_request: Some(HttpRequest {
                        path: "https://example.test/original".into(),
                        ..Default::default()
                    }),
                }],
            })
            .unwrap();

        let view = restored_view(&state, cx);
        let original = draft_at(&view, 0, cx);
        cx.read(|cx| {
            assert_eq!(view.read(cx).tabs[0].request_id.as_deref(), captured_id);
            assert!(original.read(cx).is_dirty());
        });

        let requested_ids = Rc::new(RefCell::new(Vec::new()));
        let _subscription = cx.update(|cx| {
            let requested_ids = requested_ids.clone();
            cx.subscribe(&view, move |_, event: &RequestSaveRequested, _| {
                requested_ids
                    .borrow_mut()
                    .push(event.request_id.to_string());
            })
        });
        view.update(cx, |view, cx| view.save_active_request(cx));
        cx.run_until_parked();
        assert_eq!(
            requested_ids.borrow().as_slice(),
            captured_id.into_iter().collect::<Vec<_>>()
        );

        let replacement = collection::FileEntry::from_path(&file_path).unwrap();
        view.update(cx, |view, cx| {
            view.open_request(
                &file_path,
                replacement.id.clone().into(),
                replacement.name.clone().into(),
                "API".into(),
                &replacement.request,
                cx,
            );
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let view = view.read(cx);

            assert_eq!(view.tabs.len(), 2);
            assert_eq!(view.tabs[0].request_id.as_deref(), captured_id);
            assert_eq!(
                view.tabs[1].request_id.as_deref(),
                Some("replacement-request-id")
            );
            assert_ne!(view.tabs[0].page.entity_id(), view.tabs[1].page.entity_id());
            assert_eq!(
                original.read(cx).request.path,
                "https://example.test/original-edited"
            );
        });
        let saved = SessionStore::new(state.join("session.json"))
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(saved.tabs[0].request_id.as_deref(), captured_id);
        assert_eq!(
            saved.tabs[1].request_id.as_deref(),
            Some("replacement-request-id")
        );
    }
}
