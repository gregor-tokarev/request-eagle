use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    CollectionEditError, CollectionRegistry, Entry, FileEntry, HttpRequest, Method, Request,
};

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

#[test]
fn saving_request_preserves_latest_metadata_comments_and_unknown_fields() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    fs::write(
        &path,
        r#"# externally edited request
id = 'external-id' # stable identity
name = 'External name' # display name
schema_version = 3 # schema comment
custom = 'keep the metadata'

[request] # request comment
type = 'http'
method = 'GET' # method comment
path = '/users' # endpoint comment
body = [111, 108, 100]
query = [['old', 'value']]
request_custom = 'keep the request metadata'
"#,
    )
    .unwrap();
    let updated = HttpRequest {
        method: Method::Post,
        path: "https://example.com/v2/users".into(),
        headers: vec![("Accept".into(), "application/json".into())],
        body: Some(b"new body".to_vec()),
        query: Some(vec![("page".into(), "2".into())]),
    };

    registry.update_request(&path, (&updated).into()).unwrap();

    let cached = registry.file(&path).unwrap();
    assert_eq!(cached.id, "external-id");
    assert_eq!(cached.name, "External name");
    assert_eq!(cached.schema_version, 3);
    let loaded = FileEntry::from_path(&path).unwrap();
    let Request::Http(saved) = loaded.request;
    assert_eq!(saved.method, updated.method);
    assert_eq!(saved.path, updated.path);
    assert_eq!(saved.headers, updated.headers);
    assert_eq!(saved.body, updated.body);
    assert_eq!(saved.query, updated.query);
    let content = fs::read_to_string(&path).unwrap();
    for preserved in [
        "# externally edited request",
        "# stable identity",
        "# display name",
        "# schema comment",
        "# request comment",
        "# method comment",
        "# endpoint comment",
        "custom = 'keep the metadata'",
        "request_custom = 'keep the request metadata'",
    ] {
        assert!(content.contains(preserved), "missing {preserved}");
    }

    let cleared = HttpRequest {
        body: None,
        query: None,
        ..updated
    };
    registry.update_request(&path, cleared.into()).unwrap();
    let Request::Http(reloaded) = FileEntry::from_path(&path).unwrap().request;
    assert_eq!(reloaded.body, None);
    assert_eq!(reloaded.query, None);
}

#[test]
fn failed_request_save_keeps_file_and_registry_unchanged() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    let original = fs::read_to_string(&path).unwrap();
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();

    let result = registry.update_request(
        &path,
        HttpRequest {
            path: "https://example.com/edited".into(),
            ..HttpRequest::default()
        }
        .into(),
    );

    fs::set_permissions(&path, original_permissions).unwrap();

    assert!(matches!(result, Err(CollectionEditError::Save(_))));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let Request::Http(cached) = &registry.file(&path).unwrap().request;
    assert_eq!(cached.path, "/users");
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn saving_inline_request_keeps_unknown_fields_and_removes_cleared_body() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    fs::write(
        &path,
        "id = 'list'\nname = 'List users'\nschema_version = 1\nrequest = { type = 'http', method = 'GET', path = '/users', body = [65], custom = 'keep' } # request comment\n",
    )
    .unwrap();
    let mut registry = CollectionRegistry::from_path(root).unwrap();

    registry
        .update_request(
            &path,
            HttpRequest {
                path: "https://example.com/edited".into(),
                ..HttpRequest::default()
            }
            .into(),
        )
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("custom = 'keep'"));
    assert!(content.contains("# request comment"));
    let Request::Http(reloaded) = FileEntry::from_path(&path).unwrap().request;
    assert_eq!(reloaded.path, "https://example.com/edited");
    assert_eq!(reloaded.body, None);
}
