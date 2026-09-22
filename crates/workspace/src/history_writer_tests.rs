use std::{fs, path::Path, sync::Arc, thread};

use request::HttpRequest;

use crate::{history::HistoryEntry, history_writer::HistoryWriter};

fn entry(name: &str) -> Arc<HistoryEntry> {
    Arc::new(HistoryEntry::new(
        name.into(),
        HttpRequest {
            path: format!("https://example.test/{name}"),
            body: Some(vec![0, 1, 255]),
            ..Default::default()
        },
        None,
    ))
}

fn read_entries(path: &Path) -> Vec<HistoryEntry> {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn reordered_background_jobs_preserve_the_newest_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let writer = HistoryWriter::new(path.clone());
    let original = entry("original");
    let old = writer.checkpoint(vec![original.clone()]);
    let newest = writer.clone().checkpoint(vec![entry("newest"), original]);
    let newest_revision = newest.revision();

    assert!(newest.revision() > old.revision());
    assert_eq!(writer.committed_revision(), 0);

    newest.write().unwrap();
    old.write().unwrap();

    assert_eq!(writer.committed_revision(), newest_revision);

    let restored = read_entries(&path);

    assert_eq!(restored.len(), 2);
    assert_eq!(restored[0].name, "newest");
    assert_eq!(restored[1].name, "original");
    assert_eq!(restored[0].request.body, Some(vec![0, 1, 255]));
}

#[test]
fn pending_append_cannot_restore_cleared_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let writer = HistoryWriter::new(path.clone());

    writer.flush(vec![entry("saved")]).unwrap();

    let pending = writer.checkpoint(vec![entry("pending"), entry("saved")]);
    let clear = writer.checkpoint(Vec::new());

    clear.write().unwrap();
    pending.write().unwrap();

    assert!(read_entries(&path).is_empty());
}

#[test]
fn renamed_environment_survives_a_late_older_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let writer = HistoryWriter::new(path.clone());
    let old_environment = directory.path().join("Old API/environment.toml");
    let new_environment = directory.path().join("Renamed API/environment.toml");
    let original = Arc::new(HistoryEntry::new(
        "Request".into(),
        HttpRequest::default(),
        Some(old_environment),
    ));
    let pending = writer.checkpoint(vec![original.clone()]);
    let mut renamed = original.as_ref().clone();
    renamed.environment_path = Some(new_environment.clone());

    writer.checkpoint(vec![Arc::new(renamed)]).write().unwrap();
    pending.write().unwrap();

    assert_eq!(
        read_entries(&path)[0].environment_path,
        Some(new_environment)
    );
}

#[test]
fn final_flush_survives_concurrent_and_late_history_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let writer = HistoryWriter::new(path.clone());
    let late = writer.checkpoint(vec![entry("late")]);
    let pending: Vec<_> = (0..16)
        .map(|index| writer.checkpoint(vec![entry(&format!("queued-{index}"))]))
        .collect();

    thread::scope(|scope| {
        for job in pending {
            scope.spawn(move || job.write().unwrap());
        }

        writer.flush(vec![entry("final")]).unwrap();
    });

    late.write().unwrap();

    assert_eq!(read_entries(&path)[0].name, "final");
}

#[test]
fn unavailable_storage_preserves_prior_history_and_allows_retry() {
    let directory = tempfile::tempdir().unwrap();
    let storage = directory.path().join("storage");
    let preserved = directory.path().join("preserved");
    let path = storage.join("history.json");
    let writer = HistoryWriter::new(path.clone());

    writer.flush(vec![entry("original")]).unwrap();

    let original = fs::read(&path).unwrap();

    // Make the storage directory unavailable without relying on permissions,
    // which would not fail when these tests run as root.
    fs::rename(&storage, &preserved).unwrap();
    fs::write(&storage, b"blocked").unwrap();

    assert!(writer.checkpoint(vec![entry("failed")]).write().is_err());
    assert_eq!(fs::read(preserved.join("history.json")).unwrap(), original);

    fs::remove_file(&storage).unwrap();
    fs::rename(&preserved, &storage).unwrap();
    writer.flush(vec![entry("retried")]).unwrap();

    assert_eq!(read_entries(&path)[0].name, "retried");
}

#[test]
fn failed_newer_append_leaves_a_successful_clear_committed() {
    let directory = tempfile::tempdir().unwrap();
    let storage = directory.path().join("storage");
    let preserved = directory.path().join("preserved");
    let path = storage.join("history.json");
    let writer = HistoryWriter::new(path.clone());

    writer.flush(vec![entry("original")]).unwrap();

    let clear = writer.checkpoint(Vec::new());
    let clear_revision = clear.revision();

    clear.write().unwrap();

    assert_eq!(writer.committed_revision(), clear_revision);
    assert!(read_entries(&path).is_empty());

    fs::rename(&storage, &preserved).unwrap();
    fs::write(&storage, b"blocked").unwrap();

    let append = writer.checkpoint(vec![entry("failed")]);

    assert!(append.revision() > clear_revision);
    assert!(append.write().is_err());
    assert_eq!(writer.committed_revision(), clear_revision);
    assert!(read_entries(&preserved.join("history.json")).is_empty());
}
