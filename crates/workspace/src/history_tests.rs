use std::{fs, path::Path, sync::Arc};

use request::HttpRequest;

use crate::history::{History, HistoryEntry};

fn entry(name: &str, root: Option<&Path>) -> HistoryEntry {
    HistoryEntry::new(
        name.into(),
        HttpRequest {
            path: format!("https://example.test/{name}"),
            body: Some(vec![7; 256 * 1024]),
            ..Default::default()
        },
        root.map(|root| root.join("environment.toml")),
    )
}

#[test]
fn history_keeps_latest_snapshots_and_can_be_cleared() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let mut history = History::load(path.clone()).unwrap();

    for index in 0..105 {
        history.push(HistoryEntry::new(
            index.to_string(),
            HttpRequest::default(),
            None,
        ));
    }

    assert!(
        !path.exists(),
        "mutating history must not write on the caller thread"
    );
    history.flush().unwrap();
    let mut reloaded = History::load(path.clone()).unwrap();
    assert_eq!(reloaded.entries.len(), 100);
    assert_eq!(reloaded.entries[0].name, "104");
    assert_eq!(reloaded.entries[99].name, "5");
    assert!(reloaded.clear());
    reloaded.flush().unwrap();
    assert!(!reloaded.is_clearing());
    assert!(History::load(path).unwrap().entries.is_empty());
}

#[test]
fn corrupt_history_is_not_silently_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    fs::write(&path, b"not json").unwrap();

    assert!(History::load(path.clone()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"not json");
}

#[test]
fn queueing_and_relocating_history_share_request_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let before = directory.path().join("API");
    let after = directory.path().join("api");
    fs::create_dir(&before).unwrap();
    let mut history = History::load(directory.path().join("history.json")).unwrap();
    history.push(entry("one", Some(&before)));
    let previous = history.entries[0].clone();
    let queued = history.checkpoint().unwrap();

    // The old path still exists, as on a case-insensitive collection rename.
    assert!(history.relocate_environment(&before, &after.join("environment.toml")));
    assert!(Arc::ptr_eq(&previous.request, &history.entries[0].request));
    assert_ne!(
        previous.environment_path,
        history.entries[0].environment_path
    );
    assert!(
        !history.relocate_environment(&before.join("item.toml"), &after.join("environment.toml"))
    );
    history.flush().unwrap();
    queued.write().unwrap();
    assert_eq!(
        History::load(directory.path().join("history.json"))
            .unwrap()
            .entries[0]
            .environment_path,
        Some(after.join("environment.toml"))
    );
}

#[test]
fn failed_clear_restores_old_entries_after_new_attempts_with_updated_bindings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let before = directory.path().join("API");
    let after = directory.path().join("Renamed API");
    let mut history = History::load(path.clone()).unwrap();
    history.push(entry("old", Some(&before)));
    history.flush().unwrap();
    let old_payload = history.entries[0].request.clone();

    assert!(history.clear());
    let clear = history.checkpoint().unwrap();
    assert!(history.relocate_environment(&before, &after.join("environment.toml")));
    history.push(entry("new", Some(&after)));
    let latest = history.checkpoint().unwrap();
    let revision = latest.revision();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    let result = latest.write();
    assert!(result.is_err());
    assert!(history.finish_write(revision, false));
    assert!(!history.is_clearing());
    assert_eq!(
        history
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["new", "old"]
    );
    assert!(
        history
            .entries
            .iter()
            .all(|entry| entry.environment_path == Some(after.join("environment.toml")))
    );
    assert!(Arc::ptr_eq(&old_payload, &history.entries[1].request));
    clear.write().unwrap();

    fs::remove_dir(&path).unwrap();
    history.flush().unwrap();
    assert_eq!(History::load(path).unwrap().entries.len(), 2);
}

#[test]
fn committed_clear_is_not_rolled_back_when_a_newer_append_fails() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let mut history = History::load(path.clone()).unwrap();
    history.push(entry("old", None));
    history.flush().unwrap();
    history.clear();
    let clear = history.checkpoint().unwrap();
    clear.write().unwrap();

    // Deliberately leave the Clear completion callback pending while sending.
    history.push(entry("new", None));
    let latest = history.checkpoint().unwrap();
    let revision = latest.revision();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(latest.write().is_err());
    history.finish_write(revision, false);

    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].name, "new");
    assert!(!history.is_clearing());
}

#[test]
fn final_flush_commits_pending_clear_and_new_attempt_before_older_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let before = directory.path().join("API");
    let after = directory.path().join("Renamed API");
    let mut history = History::load(path.clone()).unwrap();
    history.push(entry("old", Some(&before)));
    let old = history.checkpoint().unwrap();
    history.clear();
    let clear = history.checkpoint().unwrap();
    history.relocate_environment(&before, &after.join("environment.toml"));
    history.push(entry("new", Some(&after)));
    history.flush().unwrap();
    old.write().unwrap();
    clear.write().unwrap();

    let recovered = History::load(path).unwrap();
    assert_eq!(recovered.entries.len(), 1);
    assert_eq!(recovered.entries[0].name, "new");
    assert_eq!(
        recovered.entries[0].environment_path,
        Some(after.join("environment.toml"))
    );
    assert!(!history.is_clearing());
}
