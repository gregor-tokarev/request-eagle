use std::{fs, io, sync::Arc, thread};

use collection::HttpRequest;

use crate::session::{RecoveredTab, SessionCheckpoint, SessionStore, SessionWriter};

fn checkpoint(name: &str) -> SessionCheckpoint {
    SessionCheckpoint {
        tabs: vec![Arc::new(RecoveredTab {
            title: name.into(),
            name: name.into(),
            collection: None,
            request_path: None,
            request_id: None,
            environment_path: None,
            request: HttpRequest {
                path: format!("https://example.test/{name}"),
                body: Some(vec![0, 1, 255]),
                ..Default::default()
            },
            saved_request: None,
        })],
        selected: Some(0),
    }
}

#[test]
fn background_checkpoints_use_the_existing_recovery_format() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let writer = SessionWriter::new(SessionStore::new(&path));
    let checkpoint = checkpoint("edited");

    writer.checkpoint(checkpoint.clone()).write().unwrap();

    let restored = SessionStore::new(&path).load().unwrap().unwrap();

    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(checkpoint).unwrap()
    );
}

#[test]
fn a_late_older_job_cannot_replace_a_newer_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let writer = SessionWriter::new(SessionStore::new(&path));
    let old = writer.checkpoint(checkpoint("old"));
    let newest = writer.clone().checkpoint(checkpoint("newest"));

    newest.write().unwrap();
    old.write().unwrap();

    let restored = SessionStore::new(&path).load().unwrap().unwrap();

    assert_eq!(restored.tabs[0].name, "newest");
}

#[test]
fn superseded_pending_checkpoints_skip_disk_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let writer = SessionWriter::new(SessionStore::new(&path));
    let old = writer.checkpoint(checkpoint("old"));
    let newest = writer.checkpoint(checkpoint("newest"));

    old.write().unwrap();

    assert!(!path.exists());

    newest.write().unwrap();

    assert_eq!(
        SessionStore::new(&path).load().unwrap().unwrap().tabs[0].name,
        "newest"
    );
}

#[test]
fn final_flush_survives_concurrent_and_late_background_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let writer = SessionWriter::new(SessionStore::new(&path));
    let late = writer.checkpoint(checkpoint("late"));
    let pending: Vec<_> = (0..32)
        .map(|index| writer.checkpoint(checkpoint(&format!("queued-{index}"))))
        .collect();

    thread::scope(|scope| {
        for job in pending {
            scope.spawn(move || job.write().unwrap());
        }

        writer.flush(checkpoint("final")).unwrap();
    });

    late.write().unwrap();

    assert_eq!(
        SessionStore::new(&path).load().unwrap().unwrap().tabs[0].name,
        "final"
    );
}

#[test]
fn failed_checkpoint_preserves_recovery_and_does_not_block_a_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let writer = SessionWriter::new(SessionStore::new(&path));

    writer.flush(checkpoint("original")).unwrap();

    let original = fs::read(&path).unwrap();
    let error = writer
        .checkpoint(SessionCheckpoint {
            tabs: Vec::new(),
            selected: Some(0),
        })
        .write()
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(fs::read(&path).unwrap(), original);

    writer.flush(checkpoint("retried")).unwrap();

    assert_eq!(
        SessionStore::new(&path).load().unwrap().unwrap().tabs[0].name,
        "retried"
    );
}
