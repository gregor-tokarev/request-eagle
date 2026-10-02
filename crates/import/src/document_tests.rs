use std::{fs, path::Path};

use crate::{CollectionImport, Import, ImportError, parse, read, sources};

/// The collection a document converts into.
pub(crate) fn parse_collection(source: &str) -> Result<CollectionImport, ImportError> {
    parse(source).map(collection)
}

/// The collection a file or folder converts into.
pub(crate) fn read_collection(path: &Path) -> Result<CollectionImport, ImportError> {
    read(path).map(collection)
}

fn collection(import: Import) -> CollectionImport {
    match import {
        Import::Collection(import) => import,
        Import::Environment(environment) => panic!("{} is an environment", environment.name),
    }
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn formats_are_recognized_by_their_content() {
    let postman = "\u{feff}{\"info\": {\"name\": \"Postman\"}, \"item\": []}";
    let openapi = "openapi: 3.1.0\ninfo:\n  title: OpenAPI\npaths: {}\n";

    let environment = "name: Staging\nvalues: []\n";

    assert_eq!(
        parse_collection(postman).unwrap().collection.name,
        "Postman"
    );
    assert_eq!(
        parse_collection(openapi).unwrap().collection.name,
        "OpenAPI"
    );
    assert!(matches!(
        parse(environment).unwrap(),
        Import::Environment(environment) if environment.name == "Staging"
    ));
}

#[test]
fn unreadable_and_unknown_files_are_explained() {
    assert!(matches!(
        parse("{\"info\": ").err().unwrap(),
        ImportError::Syntax(_)
    ));
    assert!(matches!(
        parse("paths: [unclosed").err().unwrap(),
        ImportError::Syntax(_)
    ));
    assert!(matches!(
        parse("{\"values\": []}").err().unwrap(),
        ImportError::UnknownFormat
    ));
    assert!(matches!(
        parse("{\"id\": \"1\", \"requests\": [], \"order\": []}")
            .err()
            .unwrap(),
        ImportError::PostmanV1
    ));
    // Postman writes each request of a collection folder to its own file.
    assert!(matches!(
        parse("$kind: grpc-request\nurl: localhost:50051\n")
            .err()
            .unwrap(),
        ImportError::PostmanV3File
    ));
}

#[test]
fn names_are_single_lines() {
    let import = parse_collection(
        r#"{"info": {"name": "  Pets\nAPI  "}, "item": [{"name": "", "request": {"url": "/"}}]}"#,
    )
    .unwrap();

    assert_eq!(import.collection.name, "Pets API");

    let collection::ImportedItem::Request { name, .. } = &import.collection.items[0] else {
        panic!("expected a request");
    };
    assert_eq!(name, "Untitled");
}

#[test]
fn postman_workspace_folders_import_each_collection_and_environment() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("postman");
    write(
        &workspace,
        "collections/Shop/.resources/definition.yaml",
        "$kind: collection\n",
    );
    write(
        &workspace,
        "collections/Admin/List users.request.yaml",
        "$kind: http-request\nurl: https://admin.test/users\n",
    );
    write(
        &workspace,
        "environments/Staging.environment.yaml",
        "name: Staging\nvalues: []\n",
    );
    write(
        &workspace,
        "globals/workspace.globals.yaml",
        "name: Globals\n",
    );
    write(&workspace, "specs/Shop/openapi.yaml", "openapi: 3.1.0\n");
    let expected = [
        workspace.join("collections/Admin"),
        workspace.join("collections/Shop"),
        workspace.join("environments/Staging.environment.yaml"),
    ];

    // The repository connected to Postman, its `postman` folder, and the
    // folder of collections all import the workspace.
    assert_eq!(sources(directory.path()), expected);
    assert_eq!(sources(&workspace), expected);
    assert_eq!(sources(&workspace.join("collections")), expected[..2]);

    // Anything else imports as it is.
    for path in [
        workspace.join("collections/Shop"),
        workspace.join("environments/Staging.environment.yaml"),
        workspace.join("specs"),
    ] {
        assert_eq!(sources(&path), std::slice::from_ref(&path));
    }
}
