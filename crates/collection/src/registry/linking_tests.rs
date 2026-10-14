use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{CollectionEditError, CollectionRegistry, LinkedFolder};

const REQUEST: &str = "id = 'health'\nname = 'Health'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/health'\n";

/// A data directory with an empty collections directory, and a project
/// whose `api` folder holds a collection beside the project's code.
fn fixture() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("data/collections")).unwrap();
    fs::create_dir_all(root.path().join("project/api/Users")).unwrap();
    fs::write(root.path().join("project/api/Users/Health.toml"), REQUEST).unwrap();
    // Saved when its folder was made, the order marks the collection's top.
    fs::write(
        root.path().join("project/api/.request-eagle-order.json"),
        "[\"Users\"]",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("project/src")).unwrap();
    fs::write(root.path().join("project/Cargo.toml"), "[package]\n").unwrap();
    fs::write(root.path().join("project/src/main.rs"), "fn main() {}\n").unwrap();

    root
}

fn open(root: &Path) -> CollectionRegistry {
    CollectionRegistry::open(
        root.join("data/collections"),
        root.join("data/linked-collections.json"),
    )
}

fn link(registry: &mut CollectionRegistry, path: &Path) -> Result<(), CollectionEditError> {
    registry.link(LinkedFolder::read(path)?).map(|_| ())
}

#[test]
fn linked_folders_load_with_the_collections_directory() {
    let root = fixture();
    let api = root.path().join("project/api");
    let mut registry = open(root.path());

    // However the folder is reached, it has one path.
    link(&mut registry, &root.path().join("project/src/../api")).unwrap();

    assert!(registry.is_linked(&api));
    assert!(registry.file(&api.join("Users/Health.toml")).is_some());

    let registry = open(root.path());
    let [collection] = registry.collections() else {
        panic!("expected the linked collection");
    };
    assert_eq!(collection.path, api);
    assert!(registry.is_linked(&api));
}

#[test]
fn only_empty_folders_and_collections_can_be_linked() {
    let root = fixture();
    let mut registry = open(root.path());

    // Neither the project's code nor the project, which holds a collection
    // deeper down, is a collection.
    for path in ["project/src", "project"] {
        assert!(
            matches!(
                link(&mut registry, &root.path().join(path)),
                Err(CollectionEditError::NotACollection)
            ),
            "{path}"
        );
    }

    // A new repository holds nothing but its hidden `.git` folder.
    let empty = root.path().join("empty");
    fs::create_dir_all(empty.join(".git")).unwrap();
    link(&mut registry, &empty).unwrap();

    // Variables alone make a collection.
    let variables = root.path().join("variables");
    fs::create_dir_all(&variables).unwrap();
    fs::write(variables.join("environment.toml"), "token = 'secret'\n").unwrap();
    link(&mut registry, &variables).unwrap();

    assert_eq!(registry.collections().len(), 2);
}

#[test]
fn folders_that_overlap_a_collection_cannot_be_linked() {
    let root = fixture();
    let mut registry = open(root.path());
    let api = root.path().join("project/api");
    link(&mut registry, &api).unwrap();
    let inside_directory = registry.create_collection().unwrap();
    registry.create_request(&inside_directory).unwrap();

    for path in [api.clone(), api.join("Users"), inside_directory] {
        assert!(
            matches!(
                link(&mut registry, &path),
                Err(CollectionEditError::AlreadyCollection)
            ),
            "{} overlaps a collection",
            path.display()
        );
    }
}

#[test]
fn unlinking_leaves_the_folder() {
    let root = fixture();
    let api = root.path().join("project/api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();

    registry.unlink(&api).unwrap();

    assert!(registry.collections().is_empty());
    assert!(api.join("Users/Health.toml").exists());
    assert!(open(root.path()).collections().is_empty());
}

#[test]
fn a_linked_collection_is_never_deleted() {
    let root = fixture();
    let api = root.path().join("project/api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();

    assert!(matches!(
        registry.delete(&api),
        Err(CollectionEditError::LinkedCollection)
    ));
    assert!(api.join("Users/Health.toml").exists());
}

#[test]
fn renaming_a_linked_collection_moves_its_link() {
    let root = fixture();
    let api = root.path().join("project/api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();

    let renamed = registry.rename(&api, "requests").unwrap();

    assert_eq!(renamed, root.path().join("project/requests"));
    assert!(registry.is_linked(&renamed));
    let registry = open(root.path());
    assert_eq!(registry.collections()[0].path, renamed);
    assert!(registry.skipped().is_empty());
}

#[test]
fn a_missing_linked_folder_stays_linked_until_it_is_back() {
    let root = fixture();
    let api = root.path().join("project/api");
    let moved = root.path().join("api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();

    // As on an unplugged drive.
    fs::rename(&api, &moved).unwrap();
    let mut registry = open(root.path());
    assert!(registry.collections().is_empty());
    assert!(registry.is_linked(&api));
    assert!(registry.skipped().is_empty());

    fs::rename(&moved, &api).unwrap();
    assert_eq!(open(root.path()).collections()[0].path, api);

    // A folder that is gone for good can be forgotten.
    fs::remove_dir_all(&api).unwrap();
    registry.unlink(&api).unwrap();
    assert!(!open(root.path()).is_linked(&api));
}

#[test]
fn hidden_folders_are_not_collections_or_folders() {
    let root = fixture();
    let collections = root.path().join("data/collections");
    fs::create_dir_all(collections.join(".git/objects")).unwrap();
    fs::create_dir_all(collections.join("API/.github")).unwrap();
    fs::write(collections.join("API/.github/ci.toml"), "name = 'ci'\n").unwrap();
    fs::write(collections.join("API/Health.toml"), REQUEST).unwrap();

    // Hidden requests, such as those older versions saved, still load.
    fs::write(collections.join("API/.well-known.toml"), REQUEST).unwrap();

    let registry = open(root.path());

    let [collection] = registry.collections() else {
        panic!("expected only the API collection");
    };
    assert_eq!(collection.entries.len(), 2);
    assert!(registry.skipped().is_empty());
}

#[test]
fn collections_cannot_be_renamed_to_hidden_names() {
    let root = fixture();
    let mut registry = open(root.path());
    let collection = registry.create_collection().unwrap();

    assert!(matches!(
        registry.rename(&collection, ".hidden"),
        Err(CollectionEditError::HiddenName)
    ));
}

#[test]
fn reloading_counts_the_requests_changed_on_disk() {
    let root = fixture();
    let api = root.path().join("project/api");
    let health = api.join("Users/Health.toml");
    let status = api.join("Status.toml");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();
    fs::write(&status, REQUEST.replace("health", "status")).unwrap();

    registry.reload();
    assert_eq!(registry.file(&health).unwrap().outside_changes(), 0);
    assert_eq!(registry.file(&status).unwrap().outside_changes(), 0);

    fs::write(&health, REQUEST.replace("/health", "/healthz")).unwrap();
    fs::remove_file(&status).unwrap();
    registry.reload();

    assert_eq!(registry.file(&health).unwrap().outside_changes(), 1);
    assert!(registry.file(&status).is_none());
    assert!(registry.is_linked(&api));
}

#[test]
fn reloading_counts_collection_settings_changed_on_disk() {
    let root = fixture();
    let api = root.path().join("project/api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();
    registry.reload();
    assert_eq!(registry.collections()[0].outside_changes(), 0);

    fs::write(api.join("environment.toml"), "base = 'remote'\n").unwrap();
    registry.reload();

    let [collection] = registry.collections() else {
        panic!("expected the linked collection");
    };
    assert_eq!(collection.outside_changes(), 1);
    assert_eq!(collection.local_env().resolve("base"), Some("remote"));
}

#[test]
fn a_linked_folder_that_is_back_is_noticed() {
    let root = fixture();
    let api = root.path().join("project/api");
    let moved = root.path().join("api");
    let mut registry = open(root.path());
    link(&mut registry, &api).unwrap();
    fs::rename(&api, &moved).unwrap();
    let registry = open(root.path());
    assert!(!registry.linked_folder_returned());

    fs::rename(&moved, &api).unwrap();

    assert!(registry.linked_folder_returned());
}
