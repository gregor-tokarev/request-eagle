use std::{collections::HashSet, fs, path::PathBuf};

use request::Authentication;
use uuid::Uuid;

use crate::{CollectionRegistry, Entry, HttpRequest, ImportedRequest, Method, Request};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("request-eagle-import-{}", Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }

        let _ = fs::remove_dir_all(&self.0);
    }
}

fn imported_request(name: &str, folders: &[&str], path: &str) -> ImportedRequest {
    ImportedRequest {
        name: name.into(),
        folders: folders.iter().map(|name| (*name).to_owned()).collect(),
        request: HttpRequest {
            path: path.into(),
            ..HttpRequest::default()
        },
    }
}

#[test]
fn imported_requests_reload_with_all_request_fields_and_authentication() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let request = HttpRequest {
        method: Method::Post,
        path: "{{base_url}}/users?enabled=true".into(),
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("X-Trace".into(), "first".into()),
            ("X-Trace".into(), "second".into()),
        ],
        body: Some(br#"{"name":"Ada"}"#.to_vec()),
        query: Some(vec![("page".into(), "2".into())]),
        authentication: Authentication::Bearer {
            token: "{{api_token}}".into(),
        },
        ..Default::default()
    };
    let expected = serde_json::to_value(&request).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![ImportedRequest {
                name: "Create user".into(),
                folders: Vec::new(),
                request,
            }],
        )
        .unwrap();

    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].name, "Create user");
    assert_eq!(
        serde_json::to_value(&imported[0].request).unwrap(),
        expected
    );
    assert_eq!(
        imported[0].path.parent(),
        Some(imported[0].collection_path.as_path())
    );
    assert!(registry.file(&imported[0].path).is_some());

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let file = reloaded.file(&imported[0].path).unwrap();
    let Request::Http(request) = &file.request;

    assert_eq!(reloaded.len(), 1);
    assert_eq!(file.name, "Create user");
    assert_eq!(file.schema_version, 1);
    assert!(Uuid::parse_str(&file.id).is_ok());
    assert_eq!(serde_json::to_value(request).unwrap(), expected);
}

#[test]
fn import_preserves_nested_folders_and_reuses_matching_folder_paths() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![
                imported_request(
                    "List users",
                    &["Users", "Admin"],
                    "https://example.com/users",
                ),
                imported_request(
                    "Create user",
                    &["Users", "Admin"],
                    "https://example.com/create",
                ),
                imported_request("Health", &[], "https://example.com/health"),
            ],
        )
        .unwrap();

    let collection = &imported[0].collection_path;
    assert_eq!(
        imported[0].path.parent(),
        Some(collection.join("Users/Admin").as_path())
    );
    assert_eq!(imported[0].path.parent(), imported[1].path.parent());
    assert_eq!(imported[2].path.parent(), Some(collection.as_path()));

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let entries = &reloaded.collections()[0].entries;
    let users = entries
        .iter()
        .find_map(|entry| match entry {
            Entry::Directory(folder) if folder.name == "Users" => Some(folder),
            _ => None,
        })
        .unwrap();
    let Entry::Directory(admin) = &users.entries[0] else {
        panic!("expected the nested Admin folder");
    };

    assert_eq!(users.entries.len(), 1);
    assert_eq!(admin.name, "Admin");
    assert_eq!(admin.entries.len(), 2);
    assert_eq!(entries.len(), 2);
}

#[test]
fn duplicate_names_and_sanitized_collisions_keep_distinct_requests() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![
                imported_request("Fetch", &[], "https://example.com/one"),
                imported_request("Fetch", &[], "https://example.com/two"),
                imported_request("a/b", &[], "https://example.com/three"),
                imported_request("a\\b", &[], "https://example.com/four"),
                imported_request("environment", &[], "https://example.com/five"),
            ],
        )
        .unwrap();

    let paths: HashSet<_> = imported.iter().map(|file| &file.path).collect();
    assert_eq!(paths.len(), imported.len());

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let mut ids = HashSet::new();

    for imported in &imported {
        assert_eq!(
            imported.path.parent(),
            Some(imported.collection_path.as_path())
        );
        assert_ne!(imported.path.file_name().unwrap(), "environment.toml");

        let file = reloaded.file(&imported.path).unwrap();
        let Request::Http(request) = &file.request;

        assert_eq!(file.name, imported.name);
        assert_eq!(request.path, imported.request.path);
        assert!(ids.insert(&file.id));
    }
}

#[test]
fn folder_names_that_sanitize_alike_do_not_merge() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![
                imported_request("Fetch", &["a/b"], "https://example.com/one"),
                imported_request("Fetch", &["a\\b"], "https://example.com/two"),
                imported_request("Again", &["a/b"], "https://example.com/three"),
            ],
        )
        .unwrap();

    assert_ne!(imported[0].path.parent(), imported[1].path.parent());
    assert_eq!(imported[0].path.parent(), imported[2].path.parent());

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(reloaded.collections()[0].entries.len(), 2);

    for file in imported {
        assert!(file.path.starts_with(&file.collection_path));
        assert!(reloaded.file(&file.path).is_some());
    }
}

#[test]
fn importing_again_creates_a_new_collection_without_overwriting_occupied_paths() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(fixture.0.join("Team API"), "keep this file").unwrap();

    let first = registry
        .import_requests(
            "Team API",
            vec![imported_request("Fetch", &[], "https://example.com/one")],
        )
        .unwrap();
    let first_content = fs::read(&first[0].path).unwrap();
    let second = registry
        .import_requests(
            "Team API",
            vec![imported_request("Fetch", &[], "https://example.com/two")],
        )
        .unwrap();

    assert_ne!(first[0].collection_path, second[0].collection_path);
    assert_eq!(
        fs::read_to_string(fixture.0.join("Team API")).unwrap(),
        "keep this file"
    );
    assert_eq!(fs::read(&first[0].path).unwrap(), first_content);
    assert_eq!(CollectionRegistry::from_path(&fixture.0).unwrap().len(), 2);
}

#[test]
fn empty_import_does_not_create_a_directory_or_change_the_registry() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();

    assert!(registry.import_requests("Empty", Vec::new()).is_err());
    assert!(registry.is_empty());
    assert!(!fixture.0.exists());
}

#[test]
fn failed_staging_removes_every_request_without_publishing_a_collection() {
    let fixture = Fixture::new();
    let collections = fixture.0.join("collections");
    let mut registry = CollectionRegistry::from_path(&collections).unwrap();
    let too_deep = ImportedRequest {
        name: "Cannot save".into(),
        folders: vec!["a".repeat(180); 40],
        request: HttpRequest::default(),
    };

    assert!(
        registry
            .import_requests(
                "Team API",
                vec![
                    imported_request("First", &[], "https://example.com"),
                    too_deep
                ],
            )
            .is_err()
    );
    assert!(registry.is_empty());
    assert_eq!(fs::read_dir(&collections).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn import_does_not_replace_an_existing_empty_collection_directory() {
    let fixture = Fixture::new();
    let occupied = fixture.0.join("Team API");
    fs::create_dir_all(&occupied).unwrap();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![imported_request("Fetch", &[], "https://example.com")],
        )
        .unwrap();

    assert_ne!(imported[0].collection_path, occupied);
    assert_eq!(fs::read_dir(occupied).unwrap().count(), 0);
    assert_eq!(registry.len(), 2);
}

#[cfg(unix)]
#[test]
fn import_only_requires_write_access_to_the_collections_directory() {
    use std::{io, os::unix::fs::PermissionsExt};

    let fixture = Fixture::new();
    let collections = fixture.0.join("collections");
    fs::create_dir_all(&collections).unwrap();
    fs::set_permissions(&collections, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o500)).unwrap();

    // Root can bypass mode bits. Check the effective restriction before claiming coverage.
    match fs::write(fixture.0.join("parent-write-probe"), b"probe") {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
        Ok(()) => {
            eprintln!("Skipping permission regression: this account can write through mode 0500.");
            return;
        }
        Err(error) => panic!("unexpected permission probe failure: {error}"),
    }

    let mut registry = CollectionRegistry::from_path(&collections).unwrap();
    registry.create_collection().unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![imported_request("Fetch", &[], "https://example.com")],
        )
        .unwrap();
    let reloaded = CollectionRegistry::from_path(&collections).unwrap();

    assert_eq!(reloaded.len(), 2);
    assert!(reloaded.file(&imported[0].path).is_some());
    assert_eq!(fs::read_dir(&collections).unwrap().count(), 2);
}

#[test]
fn registry_ignores_staging_directories_but_loads_other_hidden_collections() {
    let fixture = Fixture::new();
    let staging = fixture
        .0
        .join(format!(".request-eagle-import-{}", Uuid::new_v4()));
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("request.toml"), "partially written request").unwrap();
    fs::create_dir(fixture.0.join(".drafts")).unwrap();
    fs::create_dir(fixture.0.join(".request-eagle-import-not-a-uuid")).unwrap();

    let registry = CollectionRegistry::from_path(&fixture.0).unwrap();

    assert_eq!(registry.len(), 2);
    assert!(
        registry
            .collections()
            .iter()
            .all(|collection| collection.path != staging)
    );
}

#[test]
fn collection_rename_cannot_hide_requests_under_a_reserved_staging_name() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = registry
        .import_requests(
            "Team API",
            vec![imported_request("Fetch", &[], "https://example.com")],
        )
        .unwrap();
    let reserved_name = format!(".request-eagle-import-{}", Uuid::new_v4());

    assert!(
        registry
            .rename(&imported[0].collection_path, &reserved_name)
            .is_err()
    );
    assert!(
        CollectionRegistry::from_path(&fixture.0)
            .unwrap()
            .file(&imported[0].path)
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn imported_files_and_new_directories_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    registry
        .import_requests(
            "Team API",
            vec![imported_request(
                "Fetch",
                &["Users", "Admin"],
                "https://example.com",
            )],
        )
        .unwrap();

    let mut paths = vec![fixture.0.clone()];

    while let Some(path) = paths.pop() {
        let metadata = fs::metadata(&path).unwrap();

        assert_eq!(
            metadata.permissions().mode() & 0o077,
            0,
            "{} must be private",
            path.display()
        );

        if metadata.is_dir() {
            paths.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        }
    }
}
