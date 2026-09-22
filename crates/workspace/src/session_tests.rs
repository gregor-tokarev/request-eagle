use std::{fs, io};

use collection::{HttpRequest, Method};
use serde::Serialize;

use crate::session::{RecoveredTab, SessionSnapshot, SessionStore, write_private_json};

fn saved_tab(directory: &std::path::Path) -> RecoveredTab {
    let baseline = HttpRequest {
        method: Method::Post,
        path: "https://example.test/original".into(),
        headers: vec![("Authorization".into(), "Bearer secret".into())],
        body: Some(br#"{"original":true}"#.to_vec()),
        query: Some(vec![("saved".into(), "value".into())]),
        ..Default::default()
    };
    let mut edited = baseline.clone();
    edited.path = "https://example.test/edited".into();
    edited.body = Some(br#"{"edited":true}"#.to_vec());

    RecoveredTab {
        title: "Saved request".into(),
        name: "Saved request".into(),
        collection: Some("API".into()),
        request_path: Some(directory.join("API/request.toml")),
        environment_path: Some(directory.join("API/environment.toml")),
        request: edited,
        saved_request: Some(baseline),
    }
}

#[test]
fn recovery_preserves_saved_and_untitled_edits_and_active_tab() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let snapshot = SessionSnapshot {
        tabs: vec![
            saved_tab(directory.path()),
            RecoveredTab {
                title: "Untitled 2".into(),
                name: "Untitled Request".into(),
                collection: None,
                request_path: None,
                environment_path: None,
                request: HttpRequest {
                    path: "https://example.test/unsaved".into(),
                    body: Some(vec![0, 1, 255]),
                    ..Default::default()
                },
                saved_request: None,
            },
        ],
        selected: Some(1),
    };

    SessionStore::new(&path).save(&snapshot).unwrap();

    // Construct another store to exercise the same boundary as app restart.
    let restored = SessionStore::new(&path).load().unwrap().unwrap();

    assert_eq!(restored.selected, Some(1));
    assert_eq!(restored.tabs.len(), 2);
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&snapshot).unwrap()
    );
    assert_eq!(
        restored.tabs[0].saved_request.as_ref().unwrap().path,
        "https://example.test/original"
    );
    assert_eq!(restored.tabs[0].request.path, "https://example.test/edited");
    assert!(restored.tabs[1].request_path.is_none());
}

#[test]
fn missing_session_and_deliberately_closed_tabs_are_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let store = SessionStore::new(directory.path().join("session.json"));

    assert!(store.load().unwrap().is_none());

    store.save(&SessionSnapshot::default()).unwrap();

    let empty = store.load().unwrap().unwrap();

    assert!(empty.tabs.is_empty());
    assert!(empty.selected.is_none());
}

#[test]
fn corrupt_or_newer_session_files_are_preserved_for_manual_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");

    for contents in [
        "{unfinished",
        r#"{"version":99,"session":{"tabs":[],"selected":null}}"#,
        r#"{"version":1,"session":{"tabs":[],"selected":0}}"#,
    ] {
        fs::write(&path, contents).unwrap();

        assert!(SessionStore::new(&path).load().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
}

#[test]
fn invalid_snapshot_does_not_replace_previous_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let store = SessionStore::new(&path);

    store.save(&SessionSnapshot::default()).unwrap();

    let original = fs::read(&path).unwrap();
    let result = store.save(&SessionSnapshot {
        tabs: Vec::new(),
        selected: Some(0),
    });

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn serialization_failure_does_not_replace_previous_file() {
    struct Invalid;

    impl Serialize for Invalid {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("cannot serialize"))
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");

    fs::write(&path, "original").unwrap();

    assert!(write_private_json(&path, &Invalid).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn replacement_failure_cleans_up_temporary_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");

    // A directory at the target path cannot be replaced with a regular file.
    fs::create_dir(&path).unwrap();

    assert!(write_private_json(&path, &SessionSnapshot::default()).is_err());
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn recovery_files_and_new_directories_are_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("new-directory");
    let path = parent.join("session.json");
    let store = SessionStore::new(&path);

    store.save(&SessionSnapshot::default()).unwrap();

    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
        0o700
    );

    // Replacing an older file also removes any old world-readable permissions.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    store.save(&SessionSnapshot::default()).unwrap();

    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(parent).unwrap().count(), 1);
}
