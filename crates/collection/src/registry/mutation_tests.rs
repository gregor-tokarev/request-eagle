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
id = 'list' # stable identity
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
        form: None,
        query: Some(vec![("page".into(), "2".into())]),
        ..Default::default()
    };

    registry
        .update_request(&path, "list", (&updated).into())
        .unwrap();

    let cached = registry.file(&path).unwrap();
    assert_eq!(cached.id, "list");
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
    registry
        .update_request(&path, "list", cleared.into())
        .unwrap();
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
        "list",
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
fn stale_save_cannot_overwrite_a_request_recreated_at_the_same_path() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let path = registry.create_request(&root.join("API/Users")).unwrap();
    let original_id = registry.file(&path).unwrap().id.clone();

    registry.delete(&path).unwrap();
    let replacement_path = registry.create_request(&root.join("API/Users")).unwrap();
    assert_eq!(replacement_path, path);
    let replacement_id = registry.file(&path).unwrap().id.clone();
    assert_ne!(replacement_id, original_id);
    let replacement_content = fs::read_to_string(&path).unwrap();

    let result = registry.update_request(
        &path,
        &original_id,
        HttpRequest {
            path: "https://example.com/stale-edits".into(),
            ..HttpRequest::default()
        }
        .into(),
    );

    assert!(matches!(result, Err(CollectionEditError::RequestReplaced)));
    assert_eq!(fs::read_to_string(&path).unwrap(), replacement_content);
    let cached = registry.file(&path).unwrap();
    assert_eq!(cached.id, replacement_id);
    assert_eq!(cached.raw_content, replacement_content);
    let Request::Http(request) = &cached.request;
    assert_eq!(request.path, "/");
}

#[test]
fn stale_save_cannot_overwrite_an_externally_replaced_request() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let original_content = fs::read_to_string(&path).unwrap();
    let replacement_id = uuid::Uuid::new_v4().to_string();
    let replacement_content = original_content
        .replace("id = 'list'", &format!("id = '{replacement_id}'"))
        .replace("path = '/users'", "path = '/replacement'");
    fs::write(&path, &replacement_content).unwrap();

    let result = registry.update_request(
        &path,
        "list",
        HttpRequest {
            path: "https://example.com/stale-edits".into(),
            ..HttpRequest::default()
        }
        .into(),
    );

    assert!(matches!(result, Err(CollectionEditError::RequestReplaced)));
    assert_eq!(fs::read_to_string(&path).unwrap(), replacement_content);
    let cached = registry.file(&path).unwrap();
    assert_eq!(cached.id, "list");
    assert_eq!(cached.raw_content, original_content);
    let Request::Http(request) = &cached.request;
    assert_eq!(request.path, "/users");
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
            "list",
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

#[test]
fn request_saves_preserve_comments_inside_unchanged_and_edited_arrays() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    let arrays = r#"headers = [
  # Required response format.
  [
    'Accept', # Header name.
    'application/json', # Header value.
  ], # Required for this endpoint.
]
query = [
  ['page', '1'], # Current page.
  # Apply this filter.
  ['active', 'true'], # Only active users.
]
"#;
    fs::write(
        &path,
        format!(
            "id = 'list'\nname = 'List users'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/users'\n{arrays}"
        ),
    )
    .unwrap();
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let Request::Http(mut request) = registry.file(&path).unwrap().request.clone();
    request.path = "https://example.com/edited".into();

    registry
        .update_request(&path, "list", (&request).into())
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.ends_with(arrays),
        "array formatting changed: {content}"
    );

    request.headers[0].1 = "text/plain".into();
    request.query.as_mut().unwrap()[0].1 = "2".into();
    registry
        .update_request(&path, "list", (&request).into())
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let edited_arrays = arrays
        .replace("'application/json'", "\"text/plain\"")
        .replace("'1'", "\"2\"");
    assert!(
        content.ends_with(&edited_arrays),
        "array comments changed: {content}"
    );
    let Request::Http(reloaded) = FileEntry::from_path(&path).unwrap().request;
    assert_eq!(reloaded.headers, request.headers);
    assert_eq!(reloaded.query, request.query);
}

#[test]
fn deleting_and_reordering_rows_keeps_annotations_with_retained_keys() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let path = root.join("API/Users/list.toml");
    let arrays = r#"headers = [
  # Discarded header.
  ['Discard', 'unused'], # Discarded header explanation.
  # Accept header.
  ['Accept', 'application/json'], # Accept explanation.
  # Trace header.
  ['X-Trace', 'one'], # Trace explanation.
]
query = [
  # Discarded query.
  ['discard', 'unused'], # Discarded query explanation.
  # First tag.
  ['tag', 'a'], # First tag explanation.
  # Second tag.
  ['tag', 'b'], # Second tag explanation.
]
"#;
    fs::write(
        &path,
        format!(
            "id = 'list'\nname = 'List users'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/users'\n{arrays}"
        ),
    )
    .unwrap();
    let mut registry = CollectionRegistry::from_path(root).unwrap();
    let Request::Http(mut request) = registry.file(&path).unwrap().request.clone();
    request.headers.remove(0);
    request.query.as_mut().unwrap().remove(0);

    registry
        .update_request(&path, "list", (&request).into())
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let retained = arrays
        .lines()
        .filter(|line| !line.to_ascii_lowercase().contains("discard"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(
        content.ends_with(&retained),
        "row annotations changed: {content}"
    );

    request.headers.reverse();
    request.headers[0].1 = "two".into();
    request.query.as_mut().unwrap().reverse();
    request.query.as_mut().unwrap()[0].1 = "c".into();

    registry
        .update_request(&path, "list", (&request).into())
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let reordered = r#"headers = [
  # Trace header.
  ['X-Trace', "two"], # Trace explanation.
  # Accept header.
  ['Accept', 'application/json'], # Accept explanation.
]
query = [
  # Second tag.
  ['tag', "c"], # Second tag explanation.
  # First tag.
  ['tag', 'a'], # First tag explanation.
]
"#;
    assert!(
        content.ends_with(reordered),
        "row annotations moved: {content}"
    );
    let Request::Http(reloaded) = FileEntry::from_path(&path).unwrap().request;
    assert_eq!(reloaded.headers, request.headers);
    assert_eq!(reloaded.query, request.query);
}

#[test]
fn changing_and_clearing_authentication_removes_old_credentials_from_disk() {
    let fixture = Fixture::new();
    let path = fixture.0.join("API/Users/list.toml");
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let mut request = HttpRequest {
        path: "https://example.com/account".into(),
        authentication: request::Authentication::Basic {
            username: "old-user".into(),
            password: "old-password".into(),
        },
        ..Default::default()
    };
    registry
        .update_request(&path, "list", request.clone().into())
        .unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("old-password"));

    request.authentication = request::Authentication::Bearer {
        token: "new-token".into(),
    };
    registry
        .update_request(&path, "list", request.clone().into())
        .unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("old-user"));
    assert!(!content.contains("old-password"));
    assert!(content.contains("new-token"));

    request.authentication = request::Authentication::None;
    registry
        .update_request(&path, "list", request.into())
        .unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("authentication"));
    assert!(!content.contains("new-token"));
    let Request::Http(saved) = FileEntry::from_path(&path).unwrap().request;
    assert_eq!(saved.authentication, request::Authentication::None);
}
