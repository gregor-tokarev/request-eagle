use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::{CollectionRegistry, Entry};

use request::{
    Auth, Body, Method, OAuth2Auth, PasswordAuth, Request, WebSocketRequest, WebSocketSettings,
};

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
    assert_eq!(reloaded.collections().len(), 1);
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
    let Request::Http(http) = &file.request else {
        panic!("expected an HTTP request");
    };
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
    let request = request::HttpRequest {
        method: Method::Post,
        path: "https://example.test/new".into(),
        body: Some(Body::json("draft body")),
        ..Default::default()
    };
    for _ in 0..2 {
        let path = registry
            .create_request_with(&parent, "../Request/name", request.clone().into())
            .unwrap();
        assert_eq!(path.parent(), Some(parent.as_path()));
        let file = crate::FileEntry::from_path(path).unwrap();
        let Request::Http(saved) = file.request else {
            panic!("expected an HTTP request");
        };
        assert_eq!(saved, request);
    }
    assert!(
        registry
            .create_request_with(&parent, "  ", request.into())
            .is_err()
    );
    assert_eq!(registry.collections()[0].entries.len(), 2);
}

#[test]
fn saves_reloads_and_clears_request_scripts_without_losing_metadata() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let path = registry.create_request(&collection).unwrap();
    let original = crate::FileEntry::from_path(&path).unwrap();
    let Request::Http(mut request) = original.request else {
        panic!("expected an HTTP request");
    };
    assert!(request.scripts.is_empty());
    request.scripts.pre_request = "pm.variables.set('token', 'abc');\nconsole.log('ready');".into();
    request.scripts.post_response = "pm.test('ok', () => pm.response.to.have.status(200));".into();
    registry
        .update_request(&path, &original.id, Request::Http(request.clone()))
        .unwrap();
    let reloaded = crate::FileEntry::from_path(&path).unwrap();
    let Request::Http(saved) = reloaded.request else {
        panic!("expected an HTTP request");
    };
    assert_eq!(saved.scripts, request.scripts);
    assert_eq!(reloaded.name, original.name);

    // Clear only one phase, then both. Stale TOML fields must not reappear.
    request.scripts.pre_request.clear();
    registry
        .update_request(&path, &original.id, Request::Http(request.clone()))
        .unwrap();
    let Request::Http(saved) = crate::FileEntry::from_path(&path).unwrap().request else {
        panic!("expected an HTTP request");
    };
    assert_eq!(saved.scripts, request.scripts);
    request.scripts.post_response.clear();
    registry
        .update_request(&path, &original.id, Request::Http(request))
        .unwrap();
    let Request::Http(saved) = crate::FileEntry::from_path(&path).unwrap().request else {
        panic!("expected an HTTP request");
    };
    assert!(saved.scripts.is_empty());
    assert!(
        !fs::read_to_string(path)
            .unwrap()
            .contains("[request.scripts]")
    );
}

#[test]
fn websocket_requests_save_and_reload_without_stale_fields() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let mut request = WebSocketRequest {
        url: "wss://{{host}}/feed".into(),
        headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        query: vec![("room".into(), "42".into())],
        auth: Auth::OAuth2(OAuth2Auth {
            access_token: "{{token}}".into(),
            client_id: "app".into(),
            ..OAuth2Auth::default()
        }),
        message: "{\"subscribe\":\"prices\"}".into(),
        settings: WebSocketSettings {
            timeout_ms: Some(0),
            verify_certificates: Some(false),
        },
    };
    let path = registry
        .create_request_with(&collection, "Prices", request.clone().into())
        .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("type = \"websocket\""), "{content}");
    let file = crate::FileEntry::from_path(&path).unwrap();
    let Request::WebSocket(saved) = &file.request else {
        panic!("expected a WebSocket request");
    };
    assert_eq!(saved, &request);
    assert_eq!(file.request.label(), "WS");
    assert_eq!(file.request.url(), "wss://{{host}}/feed");

    // Emptied fields are removed from the file rather than kept from before.
    request.headers.clear();
    request.query.clear();
    request.message.clear();
    request.settings.timeout_ms = None;
    // Another kind of authorization keeps none of the previous one's fields.
    request.auth = Auth::Basic(PasswordAuth {
        username: "user".into(),
        password: String::new(),
    });
    registry
        .update_request(&path, &file.id, request.clone().into())
        .unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("verify_certificates = false"), "{content}");
    assert!(content.contains("type = \"basic\""), "{content}");
    for field in [
        "headers",
        "query",
        "message",
        "timeout_ms",
        "access_token",
        "client_id",
    ] {
        assert!(!content.contains(field), "{field} remained in {content}");
    }
    let Request::WebSocket(saved) = crate::FileEntry::from_path(&path).unwrap().request else {
        panic!("expected a WebSocket request");
    };
    assert_eq!(saved, request);
}
