use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{CollectionRegistry, Entry};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "request-eagle-edits-{}-{}-{}",
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("API/Users")).unwrap();
        fs::create_dir_all(root.join("Other")).unwrap();
        fs::write(
            root.join("API/environment.toml"),
            "base_url = 'https://example.com'\n",
        )
        .unwrap();
        fs::write(root.join("API/Users/list.toml"), "# keep this comment\nid = 'list'\nname = 'List users'\nschema_version = 1\ncustom = 'keep'\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/users'\n").unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rename_preserves_request_content_and_rebases_nested_paths_and_environment() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let request = root.join("API/Users/list.toml");
    // Changes made after loading must survive the rename too.
    let source = fs::read_to_string(&request)
        .unwrap()
        .replace("custom = 'keep'", "custom = 'external edit'");
    fs::write(&request, source).unwrap();
    registry.rename(&request, "All users").unwrap();
    let saved = fs::read_to_string(&request).unwrap();
    assert!(saved.contains("# keep this comment"));
    assert!(saved.contains("external edit"));
    assert!(saved.contains("All users"));

    registry.rename(&root.join("API/Users"), "People").unwrap();
    registry.rename(&root.join("API"), "Renamed API").unwrap();
    let collection = &registry.collections()[0];
    assert_eq!(
        collection.local_env().path,
        root.join("Renamed API/environment.toml")
    );
    let Entry::Directory(folder) = &collection.entries[0] else {
        panic!()
    };
    let Entry::File(file) = &folder.entries[0] else {
        panic!()
    };
    assert_eq!(file.path, root.join("Renamed API/People/list.toml"));
    assert_eq!(file.name, "All users");
    assert!(file.path.is_file());
    assert!(!root.join("API").exists());
    registry
        .rename(&file.path.clone(), "Renamed again")
        .unwrap();
    registry
        .rename(&root.join("Renamed API"), "RENAMED API")
        .unwrap();
    assert_eq!(registry.collections()[0].path, root.join("RENAMED API"));
    assert_eq!(CollectionRegistry::from_path(root).unwrap().len(), 2);
}

#[test]
fn invalid_colliding_and_failed_edits_leave_the_model_unchanged() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    for name in ["", "..", "../Outside", "Other", "bad\nname"] {
        assert!(registry.rename(&root.join("API"), name).is_err());
        assert_eq!(registry.collections()[0].path, root.join("API"));
        assert!(root.join("API/Users/list.toml").exists());
    }
    let request = root.join("API/Users/list.toml");
    fs::remove_file(&request).unwrap();
    assert!(registry.delete(&request).is_err());
    assert!(registry.rename(&request, "Missing").is_err());
    let Entry::Directory(folder) = &registry.collections()[0].entries[0] else {
        panic!()
    };
    let Entry::File(file) = &folder.entries[0] else {
        panic!()
    };
    assert_eq!(file.name, "List users");
}

#[test]
fn delete_persists_for_requests_folders_and_collections() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    registry.delete(&root.join("API/Users/list.toml")).unwrap();
    assert!(!root.join("API/Users/list.toml").exists());
    let Entry::Directory(folder) = &registry.collections()[0].entries[0] else {
        panic!()
    };
    assert!(folder.entries.is_empty());
    registry.delete(&root.join("API/Users")).unwrap();
    assert!(registry.collections()[0].entries.is_empty());
    registry.delete(&root.join("API")).unwrap();
    assert!(!root.join("API").exists());
    let reloaded = CollectionRegistry::from_path(root).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.collections()[0].path, root.join("Other"));
}
