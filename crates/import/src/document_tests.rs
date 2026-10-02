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
        "collections/.Hidden/.resources/definition.yaml",
        "$kind: collection\n",
    );
    write(
        &workspace,
        "environments/Staging.environment.yaml",
        "name: Staging\nvalues: []\n",
    );
    write(
        &workspace,
        "environments/DEV.ENVIRONMENT.YML",
        "name: Dev\n",
    );
    write(&workspace, "environments/.DS_Store", "");
    write(
        &workspace,
        "globals/workspace.globals.yaml",
        "name: Globals\n",
    );
    write(&workspace, "specs/Shop/openapi.yaml", "openapi: 3.1.0\n");
    let expected = [
        workspace.join("collections/.Hidden"),
        workspace.join("collections/Admin"),
        workspace.join("collections/Shop"),
        workspace.join("environments/DEV.ENVIRONMENT.YML"),
        workspace.join("environments/Staging.environment.yaml"),
    ];

    // The repository connected to Postman, its `postman` folder, and the
    // folder of collections all import the workspace.
    assert_eq!(sources(directory.path()), expected);
    assert_eq!(sources(&workspace), expected);
    assert_eq!(sources(&workspace.join("collections")), expected[..3]);

    // Anything else imports as it is.
    for path in [
        workspace.join("collections/Shop"),
        workspace.join("environments/Staging.environment.yaml"),
        workspace.join("specs"),
    ] {
        assert_eq!(sources(&path), std::slice::from_ref(&path));
    }
}

#[test]
fn workspaces_include_the_resources_their_manifest_lists() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path();
    write(
        repository,
        "postman/environments/Dev.environment.yaml",
        "name: Dev\n",
    );
    write(
        repository,
        "api/environments/Prod.environment.yaml",
        "name: Prod\n",
    );
    write(
        repository,
        "api/Orders/.resources/definition.yaml",
        "$kind: collection\n",
    );
    // Paths are relative to `.postman`, and resources in the default folders
    // may be listed too.
    write(
        repository,
        ".postman/resources.yaml",
        r#"workspace:
  id: 8d00dc0f-cd89-4d04-a828-ae8284c8b4fe
cloudResources:
  environments:
    ../postman/environments/Dev.environment.yaml: 40303981-76fce2d0
localResources:
  collections:
    - ../api/Orders
  environments:
    - ../api/environments/Prod.environment.yaml
"#,
    );

    let found = sources(repository);

    assert_eq!(found.len(), 3);
    assert_eq!(
        found[0],
        repository.join("postman/environments/Dev.environment.yaml")
    );
    assert!(found[1].ends_with("api/Orders"));
    assert!(found[2].ends_with("api/environments/Prod.environment.yaml"));
    assert_eq!(found, sources(&repository.join("postman")));
}

#[test]
fn environment_files_may_leave_out_their_variables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Empty.environment.yaml");
    fs::write(&path, "name: Empty\n").unwrap();

    assert!(matches!(
        read(&path).unwrap(),
        Import::Environment(environment)
            if environment.name == "Empty" && environment.variables.is_empty()
    ));
    // Content alone does not show that it is an environment.
    assert!(matches!(
        parse("name: Empty\n").err().unwrap(),
        ImportError::UnknownFormat
    ));

    // Variables in another shape are not dropped without a word.
    fs::write(&path, "name: Dev\nvalues:\n  token: secret\n").unwrap();
    assert!(matches!(
        read(&path).err().unwrap(),
        ImportError::UnknownFormat
    ));
}
