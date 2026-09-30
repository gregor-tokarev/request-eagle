use std::collections::HashMap;
use std::{collections::BTreeMap, path::Path};

use crate::{EnvironmentSession, EnvironmentSessions};

fn file_values() -> HashMap<String, String> {
    HashMap::from([
        ("base_url".into(), "https://example.com".into()),
        ("token".into(), "file token".into()),
    ])
}

#[test]
fn session_changes_override_file_values_without_mutating_the_source() {
    let session = EnvironmentSession::default();
    let base = file_values();
    session
        .apply(&BTreeMap::from([
            ("token".into(), Some("response token".into())),
            ("resource_id".into(), Some("42".into())),
            ("base_url".into(), None),
        ]))
        .unwrap();

    let values = session.values(base.clone());
    assert_eq!(values["token"], "response token");
    assert_eq!(values["resource_id"], "42");
    assert!(!values.contains_key("base_url"));
    assert_eq!(base["token"], "file token");
    assert_eq!(base["base_url"], "https://example.com");

    let mut refreshed = file_values();
    refreshed.insert("external_edit".into(), "new".into());
    assert_eq!(session.values(refreshed)["external_edit"], "new");
}

#[test]
fn session_registry_shares_tabs_but_isolates_collections_and_workspaces() {
    let workspace = EnvironmentSessions::default();
    let path = Path::new("/workspace/one/environment.toml");
    let login = workspace.for_path(Some(path));
    login
        .apply(&BTreeMap::from([(
            "token".into(),
            Some("login token".into()),
        )]))
        .unwrap();
    drop(login);

    let next_request = workspace.clone().for_path(Some(path));
    assert_eq!(next_request.values(file_values())["token"], "login token");
    assert_eq!(
        workspace
            .for_path(Some(Path::new("/workspace/two/environment.toml")))
            .values(file_values())["token"],
        "file token"
    );
    assert_eq!(
        workspace.for_path(None).values(file_values())["token"],
        "file token"
    );
    assert_eq!(
        EnvironmentSessions::default()
            .for_path(Some(path))
            .values(file_values())["token"],
        "file token"
    );

    workspace
        .for_path(None)
        .apply(&BTreeMap::from([(
            "token".into(),
            Some("untitled token".into()),
        )]))
        .unwrap();
    assert_eq!(
        workspace.for_path(None).values(file_values())["token"],
        "untitled token"
    );
}

#[test]
fn concurrent_requests_merge_only_their_changes() {
    let session = EnvironmentSession::default();
    let first = session.clone();
    let second = session.clone();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            first
                .apply(&BTreeMap::from([(
                    "token".into(),
                    Some("new token".into()),
                )]))
                .unwrap()
        });
        scope.spawn(move || {
            second
                .apply(&BTreeMap::from([("resource_id".into(), Some("42".into()))]))
                .unwrap()
        });
    });

    let values = session.values(file_values());
    assert_eq!(values["token"], "new token");
    assert_eq!(values["resource_id"], "42");
    assert_eq!(values["base_url"], "https://example.com");
}

#[test]
fn clearing_a_snapshot_does_not_remove_an_unseen_concurrent_value() {
    let session = EnvironmentSession::default();
    let clear_changes = session
        .values(file_values())
        .into_keys()
        .map(|key| (key, None))
        .collect();
    session
        .apply(&BTreeMap::from([(
            "new_key".into(),
            Some("concurrent".into()),
        )]))
        .unwrap();
    session.apply(&clear_changes).unwrap();

    assert_eq!(
        session.values(file_values()),
        [("new_key".into(), "concurrent".into())].into()
    );
}

#[test]
fn oversized_updates_fail_atomically_and_replacements_release_value_bytes() {
    let session = EnvironmentSession::default();
    session
        .apply(&BTreeMap::from([("token".into(), Some("valid".into()))]))
        .unwrap();

    assert!(
        session
            .apply(&BTreeMap::from([
                ("token".into(), Some("changed".into())),
                ("oversized".into(), Some("x".repeat(1024 * 1024))),
            ]))
            .is_err()
    );
    assert_eq!(
        session.values(HashMap::new()),
        [("token".into(), "valid".into())].into()
    );

    session
        .apply(&BTreeMap::from([(
            "large".into(),
            Some("x".repeat(512 * 1024)),
        )]))
        .unwrap();
    session
        .apply(&BTreeMap::from([("large".into(), None)]))
        .unwrap();
    session
        .apply(&BTreeMap::from([(
            "another".into(),
            Some("x".repeat(768 * 1024)),
        )]))
        .unwrap();
}

#[test]
fn changed_name_count_is_bounded_even_for_deleted_values() {
    let session = EnvironmentSession::default();
    let changes = (0..4097).map(|index| (index.to_string(), None)).collect();

    assert!(session.apply(&changes).is_err());
    assert!(session.values(HashMap::new()).is_empty());
}

#[test]
fn every_tab_sees_the_revision_of_committed_updates() {
    let workspace = EnvironmentSessions::default();
    let path = Path::new("/workspace/one/environment.toml");
    let sender = workspace.for_path(Some(path));
    let other_tab = workspace.for_path(Some(path));
    assert_eq!(other_tab.revision(), 0);

    sender
        .apply(&BTreeMap::from([("token".into(), Some("new".into()))]))
        .unwrap();
    assert_eq!(other_tab.revision(), 1);

    // Scripts without environment changes, like `pm.test` only, change nothing.
    sender.apply(&BTreeMap::new()).unwrap();
    assert_eq!(other_tab.revision(), 1);

    // A rejected update leaves the values, and therefore the revision, alone.
    assert!(
        sender
            .apply(&BTreeMap::from([(
                "oversized".into(),
                Some("x".repeat(1024 * 1024 + 1))
            )]))
            .is_err()
    );
    assert_eq!(other_tab.revision(), 1);
}
