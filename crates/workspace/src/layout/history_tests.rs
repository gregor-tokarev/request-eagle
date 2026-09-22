use std::{fs, path::Path, sync::Arc};

use gpui_kit::{AppContext, Entity, Modifiers, TestAppContext};

use super::{main_view::MainView, request_draft::RequestDraft};
use crate::history::{History, HistoryEntry};

fn workflow(directory: &Path, cx: &mut TestAppContext) -> (Entity<MainView>, Entity<RequestDraft>) {
    let view = cx.new(MainView::new);
    view.update(cx, |view, cx| {
        view.enable_workflow_storage(directory.to_path_buf(), cx)
    });
    let draft = cx.read(|cx| {
        view.read(cx).tabs[0]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });

    (view, draft)
}

fn attempt(name: &str, root: Option<&Path>) -> HistoryEntry {
    HistoryEntry::new(
        name.into(),
        request::HttpRequest {
            path: format!("https://example.test/{name}"),
            ..Default::default()
        },
        root.map(|root| root.join("environment.toml")),
    )
}

#[gpui_kit::test]
fn history_events_and_clear_queue_large_snapshots_without_writing_in_ui_callbacks(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let (view, draft) = workflow(directory.path(), cx);
    cx.run_until_parked();
    let large = HistoryEntry::new(
        "large".into(),
        request::HttpRequest {
            body: Some(vec![7; 256 * 1024]),
            ..Default::default()
        },
        None,
    );
    let payload = large.request.clone();
    view.update(cx, |view, _| {
        for _ in 0..100 {
            view.history.push(large.clone());
        }
    });

    // Exercise the exact subscription used by Send. The deterministic executor
    // has not polled the background job when this callback returns.
    draft.update(cx, |_, cx| cx.emit(attempt("latest", None)));
    cx.read(|cx| {
        let history = &view.read(cx).history;
        assert_eq!(history.entries.len(), 100);
        assert_eq!(history.entries[0].name, "latest");
        assert!(Arc::ptr_eq(&history.entries[1].request, &payload));
    });
    assert!(
        !path.exists(),
        "Send must not serialize or fsync history in its UI callback"
    );

    view.update(cx, |view, cx| view.clear_history(cx));
    assert!(!path.exists(), "Clear must queue its write too");
    cx.run_until_parked();

    assert!(History::load(path).unwrap().entries.is_empty());
    cx.read(|cx| {
        assert!(!view.read(cx).history.is_clearing());
        assert!(view.read(cx).history_error.is_none());
    });
}

#[gpui_kit::test]
fn history_quit_flushes_final_queued_send_when_session_recovery_is_corrupt(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session.json");
    fs::write(&session, b"broken session").unwrap();
    let (view, draft) = workflow(directory.path(), cx);
    draft.update(cx, |_, cx| cx.emit(attempt("before-clear", None)));

    cx.update(|cx| {
        view.update(cx, |view, cx| view.clear_history(cx));
        draft.update(cx, |_, cx| cx.emit(attempt("final-attempt", None)));

        // This event is still queued when quit observers are invoked.
        cx.shutdown();
    });

    let path = directory.path().join("history.json");
    let restored = History::load(path.clone()).unwrap();
    assert_eq!(restored.entries.len(), 1);
    assert_eq!(restored.entries[0].name, "final-attempt");
    assert_eq!(fs::read(session).unwrap(), b"broken session");
    cx.run_until_parked();
    assert_eq!(
        History::load(path).unwrap().entries[0].name,
        "final-attempt"
    );
    cx.read(|cx| {
        assert!(view.read(cx).storage_error.is_some());
        assert!(view.read(cx).history_error.is_none());
    });
}

#[gpui_kit::test]
fn corrupt_history_stays_disabled_while_session_recovery_remains_durable(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    fs::write(&path, b"broken history").unwrap();
    let (view, draft) = workflow(directory.path(), cx);
    draft.update(cx, |draft, cx| {
        draft.request.path = "https://example.test/unsaved".into();
        cx.emit(attempt("attempt", None));
        cx.notify();
    });
    view.update(cx, |view, cx| view.clear_history(cx));
    cx.update(|cx| cx.shutdown());
    cx.run_until_parked();

    assert_eq!(fs::read(path).unwrap(), b"broken history");
    let snapshot = crate::session::SessionStore::new(directory.path().join("session.json"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.tabs[0].request.path,
        "https://example.test/unsaved"
    );
    cx.read(|cx| {
        assert!(
            view.read(cx)
                .history_error
                .as_deref()
                .unwrap()
                .starts_with("Could not load history")
        );
        assert!(view.read(cx).storage_error.is_none());
    });
}

#[gpui_kit::test]
fn failed_history_clear_rename_and_new_attempt_restore_entries_and_report_error(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let original = directory.path().join("API");
    let renamed = directory.path().join("Renamed API");
    let (view, draft) = workflow(directory.path(), cx);
    draft.update(cx, |_, cx| cx.emit(attempt("old", Some(&original))));
    cx.run_until_parked();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    view.update(cx, |view, cx| {
        view.storage_error = Some("Unrelated session recovery error".into());
        view.clear_history(cx);
        view.relocate_history_environment(&original, &renamed.join("environment.toml"), cx);
    });
    draft.update(cx, |_, cx| cx.emit(attempt("new", Some(&renamed))));
    cx.run_until_parked();
    cx.read(|cx| {
        let view = view.read(cx);
        assert_eq!(
            view.history
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["new", "old"]
        );
        assert!(
            view.history
                .entries
                .iter()
                .all(|entry| entry.environment_path == Some(renamed.join("environment.toml")))
        );
        assert!(!view.history.is_clearing());
        assert!(view.history_error.is_some());
        assert_eq!(
            view.storage_error.as_deref(),
            Some("Unrelated session recovery error")
        );
    });

    fs::remove_dir(&path).unwrap();
    view.update(cx, |view, cx| view.save_history(cx));
    cx.run_until_parked();
    assert_eq!(History::load(path).unwrap().entries.len(), 2);
    cx.read(|cx| {
        assert!(view.read(cx).history_error.is_none());
        assert!(view.read(cx).storage_error.is_some());
    });
}

#[gpui_kit::test]
fn clear_history_button_restores_entries_and_displays_write_failure(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut view = MainView::new(cx);
        view.enable_workflow_storage(directory.path().to_path_buf(), cx);
        view.record_history(attempt("old", None), cx);
        view.history_visible = true;
        view
    });
    cx.run_until_parked();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let clear = cx.debug_bounds("clear-history").unwrap();
    cx.simulate_click(clear.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());

    assert!(cx.debug_bounds("history-storage-error").is_some());
    assert!(cx.debug_bounds("history-entry-0").is_some());
    cx.read(|cx| {
        assert_eq!(view.read(cx).history.entries[0].name, "old");
        assert!(!view.read(cx).history.is_clearing());
    });
}
