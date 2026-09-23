use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::{CollectionRegistry, Entry, Method, Request};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("request-eagle-create-{}", Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn creates_collections_folders_and_requests_that_survive_reload() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let folder = registry.create_folder(&collection).unwrap();
    let nested = registry.create_folder(&folder).unwrap();
    let request = registry.create_request(&nested).unwrap();
    let root_request = registry.create_request(&collection).unwrap();

    assert!(request.is_file());
    assert!(root_request.is_file());
    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(
        reloaded.collections()[0].local_env().path,
        collection.join("environment.toml")
    );
    let Entry::Directory(folder) = &reloaded.collections()[0].entries[0] else {
        panic!()
    };
    let Entry::Directory(nested) = &folder.entries[0] else {
        panic!()
    };
    let Entry::File(file) = &nested.entries[0] else {
        panic!()
    };
    assert_eq!(file.name, "New Request");
    assert_eq!(file.schema_version, 1);
    assert!(Uuid::parse_str(&file.id).is_ok());
    let Request::Http(http) = &file.request;
    assert!(matches!(http.method, Method::Get));
    assert_eq!(http.path, "/");
    let Entry::File(root_file) = &reloaded.collections()[0].entries[1] else {
        panic!()
    };
    assert_ne!(root_file.id, file.id);
}

#[test]
fn creation_avoids_existing_files_and_directories() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let second = registry.create_collection().unwrap();
    assert_eq!(second.file_name().unwrap(), "New Collection 2");
    fs::write(collection.join("New Folder"), "keep this file").unwrap();
    let folder = registry.create_folder(&collection).unwrap();
    assert_eq!(folder.file_name().unwrap(), "New Folder 2");
    let first = registry.create_request(&folder).unwrap();
    let original = fs::read_to_string(&first).unwrap();
    let second = registry.create_request(&folder).unwrap();
    assert_eq!(second.file_name().unwrap(), "New Request 2.toml");
    assert_eq!(fs::read_to_string(first).unwrap(), original);
    assert_eq!(
        fs::read_to_string(collection.join("New Folder")).unwrap(),
        "keep this file"
    );
}

#[test]
fn missing_or_invalid_parents_do_not_change_the_registry() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    assert!(registry.create_folder(&fixture.0.join("outside")).is_err());
    assert!(registry.create_request(&fixture.0.join("outside")).is_err());
    fs::remove_dir(&collection).unwrap();
    assert!(registry.create_folder(&collection).is_err());
    assert!(registry.create_request(&collection).is_err());
    assert!(registry.collections()[0].entries.is_empty());
    assert!(!collection.exists());
    registry.delete(&collection).unwrap_err();
}

#[test]
fn creates_a_named_request_with_its_draft_and_no_path_traversal() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let parent = registry.create_collection().unwrap();
    let request = crate::HttpRequest {
        method: Method::Post,
        path: "https://example.test/new".into(),
        body: Some(b"draft body".to_vec()),
        ..Default::default()
    };
    for _ in 0..2 {
        let path = registry
            .create_request_with(&parent, "../Request/name", request.clone().into())
            .unwrap();
        assert_eq!(path.parent(), Some(parent.as_path()));
        let file = crate::FileEntry::from_path(path).unwrap();
        let Request::Http(saved) = file.request;
        assert_eq!(saved, request);
    }
    assert!(
        registry
            .create_request_with(&parent, "  ", request.into())
            .is_err()
    );
    assert_eq!(registry.collections()[0].entries.len(), 2);
}
