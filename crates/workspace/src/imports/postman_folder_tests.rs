use collection::{CollectionRegistry, Entry, ImportedRequest};
use serde_json::json;

use super::parse_import;

#[test]
fn postman_duplicate_folders_keep_separate_groups_and_request_order() {
    let source = json!({"item": [
        {"name": "Users", "item": [
            {"name": "First user", "request": "https://example.test/first"},
            {"name": "Second user", "request": "https://example.test/second"}
        ]},
        {"name": "Health", "request": "https://example.test/health"},
        {"name": "Users", "item": [
            {"name": "Third user", "request": "https://example.test/third"}
        ]}
    ]});
    let imported = parse_import(&source.to_string()).unwrap();

    assert_eq!(
        imported
            .iter()
            .map(|request| (request.name.as_str(), request.folders.as_slice()))
            .collect::<Vec<_>>(),
        [
            ("First user", ["Users".to_owned()].as_slice()),
            ("Second user", ["Users".to_owned()].as_slice()),
            ("Health", [].as_slice()),
            ("Third user", ["Users (2)".to_owned()].as_slice())
        ]
    );

    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("collections");
    let mut registry = CollectionRegistry::from_path(&directory).unwrap();
    let saved = registry
        .import_requests(
            "Postman groups",
            imported
                .into_iter()
                .map(|request| ImportedRequest {
                    name: request.name,
                    folders: request.folders,
                    request: request.request,
                })
                .collect(),
        )
        .unwrap();

    assert_eq!(saved[0].path.parent(), saved[1].path.parent());
    assert_ne!(saved[0].path.parent(), saved[3].path.parent());

    let reloaded = CollectionRegistry::from_path(&directory).unwrap();
    let entries = &reloaded.collections()[0].entries;
    let [
        Entry::Directory(first),
        Entry::File(health),
        Entry::Directory(second),
    ] = entries.as_slice()
    else {
        panic!("expected two distinct folders with the health request between them");
    };

    assert_eq!(first.name, "Users");
    assert_eq!(health.name, "Health");
    assert_eq!(second.name, "Users (2)");

    let [Entry::File(first_user), Entry::File(second_user)] = first.entries.as_slice() else {
        panic!("expected both requests in the first source folder");
    };
    let [Entry::File(third_user)] = second.entries.as_slice() else {
        panic!("expected the final request in its own source folder");
    };

    assert_eq!(first_user.name, "First user");
    assert_eq!(second_user.name, "Second user");
    assert_eq!(third_user.name, "Third user");
}

#[test]
fn postman_duplicate_folders_reserve_literal_sibling_names() {
    let folders = ["Users", "Users", "Users (2)", "Users (3)", "Users"];
    let source = json!({"item": folders.map(|name| json!({
        "name": name,
        "item": [{"request": "https://example.test"}]
    }))});
    let imported = parse_import(&source.to_string()).unwrap();

    assert_eq!(
        imported
            .iter()
            .map(|request| request.folders[0].as_str())
            .collect::<Vec<_>>(),
        ["Users", "Users (4)", "Users (2)", "Users (3)", "Users (5)"]
    );
}

#[test]
fn postman_duplicate_folder_names_are_scoped_to_their_parent() {
    let source = json!({"item": [
        {"name": "First parent", "item": [
            {"name": "Users", "item": [{"request": "https://example.test/first"}]},
            {"name": "Users", "item": [{"request": "https://example.test/second"}]}
        ]},
        {"name": "Second parent", "item": [
            {"name": "Users", "item": [{"request": "https://example.test/third"}]}
        ]}
    ]});
    let imported = parse_import(&source.to_string()).unwrap();

    assert_eq!(imported[0].folders, ["First parent", "Users"]);
    assert_eq!(imported[1].folders, ["First parent", "Users (2)"]);
    assert_eq!(imported[2].folders, ["Second parent", "Users"]);
}

#[test]
fn postman_request_names_do_not_reserve_or_consume_folder_names() {
    let source = json!({"item": [
        {"name": "Users", "request": "https://example.test/first"},
        {"name": "Users (2)", "request": "https://example.test/second"},
        {"name": "Users", "item": [{"request": "https://example.test/third"}]},
        {"name": "Users", "item": [{"request": "https://example.test/fourth"}]},
        {"name": "Users", "request": "https://example.test/fifth"}
    ]});
    let imported = parse_import(&source.to_string()).unwrap();

    assert_eq!(imported[0].name, "Users");
    assert_eq!(imported[1].name, "Users (2)");
    assert_eq!(imported[2].folders, ["Users"]);
    assert_eq!(imported[3].folders, ["Users (2)"]);
    assert_eq!(imported[4].name, "Users");
}
