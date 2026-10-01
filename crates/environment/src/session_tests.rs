use std::collections::HashMap;
use std::{collections::BTreeMap, path::Path};

use crate::{EnvironmentSession, EnvironmentSessions, VariableScopes};

fn environment(changes: BTreeMap<String, Option<String>>) -> VariableScopes {
    VariableScopes {
        environment: changes,
        ..Default::default()
    }
}

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
        .apply(&environment(BTreeMap::from([
            ("token".into(), Some("response token".into())),
            ("resource_id".into(), Some("42".into())),
            ("base_url".into(), None),
        ])))
        .unwrap();

    let values = session.values(HashMap::new(), base.clone());
    assert_eq!(values["token"], "response token");
    assert_eq!(values["resource_id"], "42");
    assert!(!values.contains_key("base_url"));
    assert_eq!(base["token"], "file token");
    assert_eq!(base["base_url"], "https://example.com");

    let mut refreshed = file_values();
    refreshed.insert("external_edit".into(), "new".into());
    assert_eq!(
        session.values(HashMap::new(), refreshed)["external_edit"],
        "new"
    );
}

#[test]
fn session_registry_shares_tabs_but_isolates_collections_and_workspaces() {
    let workspace = EnvironmentSessions::default();
    let path = Path::new("/workspace/one/environment.toml");
    let login = workspace.for_path(Some(path));
    login
        .apply(&environment(BTreeMap::from([(
            "token".into(),
            Some("login token".into()),
        )])))
        .unwrap();
    drop(login);

    let next_request = workspace.clone().for_path(Some(path));
    assert_eq!(
        next_request.values(HashMap::new(), file_values())["token"],
        "login token"
    );
    assert_eq!(
        workspace
            .for_path(Some(Path::new("/workspace/two/environment.toml")))
            .values(HashMap::new(), file_values())["token"],
        "file token"
    );
    assert_eq!(
        workspace
            .for_path(None)
            .values(HashMap::new(), file_values())["token"],
        "file token"
    );
    assert_eq!(
        EnvironmentSessions::default()
            .for_path(Some(path))
            .values(HashMap::new(), file_values())["token"],
        "file token"
    );

    workspace
        .for_path(None)
        .apply(&environment(BTreeMap::from([(
            "token".into(),
            Some("untitled token".into()),
        )])))
        .unwrap();
    assert_eq!(
        workspace
            .for_path(None)
            .values(HashMap::new(), file_values())["token"],
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
                .apply(&environment(BTreeMap::from([(
                    "token".into(),
                    Some("new token".into()),
                )])))
                .unwrap()
        });
        scope.spawn(move || {
            second
                .apply(&environment(BTreeMap::from([(
                    "resource_id".into(),
                    Some("42".into()),
                )])))
                .unwrap()
        });
    });

    let values = session.values(HashMap::new(), file_values());
    assert_eq!(values["token"], "new token");
    assert_eq!(values["resource_id"], "42");
    assert_eq!(values["base_url"], "https://example.com");
}

#[test]
fn clearing_a_snapshot_does_not_remove_an_unseen_concurrent_value() {
    let session = EnvironmentSession::default();
    let clear_changes = session
        .values(HashMap::new(), file_values())
        .into_keys()
        .map(|key| (key, None))
        .collect();
    session
        .apply(&environment(BTreeMap::from([(
            "new_key".into(),
            Some("concurrent".into()),
        )])))
        .unwrap();
    session.apply(&environment(clear_changes)).unwrap();

    assert_eq!(
        session.values(HashMap::new(), file_values()),
        [("new_key".into(), "concurrent".into())].into()
    );
}

#[test]
fn oversized_updates_fail_atomically_and_replacements_release_value_bytes() {
    let session = EnvironmentSession::default();
    session
        .apply(&environment(BTreeMap::from([(
            "token".into(),
            Some("valid".into()),
        )])))
        .unwrap();

    assert!(
        session
            .apply(&environment(BTreeMap::from([
                ("token".into(), Some("changed".into())),
                ("oversized".into(), Some("x".repeat(1024 * 1024))),
            ])))
            .is_err()
    );
    assert_eq!(
        session.values(HashMap::new(), HashMap::new()),
        [("token".into(), "valid".into())].into()
    );

    session
        .apply(&environment(BTreeMap::from([(
            "large".into(),
            Some("x".repeat(512 * 1024)),
        )])))
        .unwrap();
    session
        .apply(&environment(BTreeMap::from([("large".into(), None)])))
        .unwrap();
    session
        .apply(&environment(BTreeMap::from([(
            "another".into(),
            Some("x".repeat(768 * 1024)),
        )])))
        .unwrap();
}

#[test]
fn changed_name_count_is_bounded_even_for_deleted_values() {
    let session = EnvironmentSession::default();
    let changes = (0..4097).map(|index| (index.to_string(), None)).collect();

    assert!(session.apply(&environment(changes)).is_err());
    assert!(session.values(HashMap::new(), HashMap::new()).is_empty());
}

#[test]
fn every_tab_sees_the_revision_of_committed_updates() {
    let workspace = EnvironmentSessions::default();
    let path = Path::new("/workspace/one/environment.toml");
    let sender = workspace.for_path(Some(path));
    let other_tab = workspace.for_path(Some(path));
    assert_eq!(other_tab.revision(), 0);

    sender
        .apply(&environment(BTreeMap::from([(
            "token".into(),
            Some("new".into()),
        )])))
        .unwrap();
    assert_eq!(other_tab.revision(), 1);

    // Scripts without environment changes, like `pm.test` only, change nothing.
    sender.apply(&environment(BTreeMap::new())).unwrap();
    assert_eq!(other_tab.revision(), 1);

    // A rejected update leaves the values, and therefore the revision, alone.
    assert!(
        sender
            .apply(&environment(BTreeMap::from([(
                "oversized".into(),
                Some("x".repeat(1024 * 1024 + 1))
            )])))
            .is_err()
    );
    assert_eq!(other_tab.revision(), 1);
}

#[test]
fn globals_are_shared_by_every_collection_of_a_workspace() {
    let workspace = EnvironmentSessions::default();
    let one = workspace.for_path(Some(Path::new("/workspace/one/environment.toml")));
    let two = workspace.for_path(Some(Path::new("/workspace/two/environment.toml")));
    let revision = two.revision();

    one.apply(&VariableScopes {
        globals: BTreeMap::from([("token".into(), Some("shared".into()))]),
        collection: BTreeMap::from([("page".into(), Some("2".into()))]),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(
        two.values(HashMap::new(), HashMap::new())["token"],
        "shared"
    );
    assert!(
        !two.values(HashMap::new(), HashMap::new())
            .contains_key("page")
    );
    assert!(two.revision() > revision);
    assert!(
        !EnvironmentSessions::default()
            .for_path(None)
            .values(HashMap::new(), HashMap::new())
            .contains_key("token")
    );

    one.apply(&VariableScopes {
        globals: BTreeMap::from([("token".into(), None)]),
        ..Default::default()
    })
    .unwrap();
    assert!(
        two.scopes(HashMap::new(), HashMap::new())
            .globals
            .is_empty()
    );
}

#[test]
fn the_environment_covers_collection_variables_which_cover_globals() {
    let session = EnvironmentSession::default();
    let collection = HashMap::from([
        ("base_url".to_owned(), "https://collection".to_owned()),
        ("page".to_owned(), "1".to_owned()),
    ]);
    let environment = HashMap::from([("base_url".to_owned(), "https://staging".to_owned())]);
    session
        .apply(&VariableScopes {
            globals: BTreeMap::from([
                ("page".into(), Some("global".into())),
                ("region".into(), Some("eu".into())),
            ]),
            collection: BTreeMap::from([("page".into(), Some("2".into()))]),
            ..Default::default()
        })
        .unwrap();

    let values = session.values(collection.clone(), environment.clone());
    assert_eq!(values["base_url"], "https://staging");
    assert_eq!(values["page"], "2");
    assert_eq!(values["region"], "eu");

    // Unsetting hides a name in its scope and the scopes beneath it.
    session
        .apply(&VariableScopes {
            collection: BTreeMap::from([("region".into(), None)]),
            environment: BTreeMap::from([("page".into(), None)]),
            ..Default::default()
        })
        .unwrap();
    let scopes = session.scopes(collection.clone(), environment.clone());
    assert_eq!(scopes.collection["page"].as_deref(), Some("2"));
    assert_eq!(scopes.environment["page"], None);

    let values = scopes.values();
    assert!(!values.contains_key("page"));
    assert!(!values.contains_key("region"));
    assert_eq!(values["base_url"], "https://staging");
}
