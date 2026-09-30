use std::{collections::HashMap, fs, path::PathBuf};

use uuid::Uuid;

use crate::{CollectionRegistry, Entry, ImportedCollection, ImportedItem};

use request::{HttpRequest, Method, Request, RequestScripts};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("request-eagle-import-{}", Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(name: &str, path: &str) -> ImportedItem {
    ImportedItem::Request {
        name: name.to_owned(),
        request: Request::Http(HttpRequest {
            method: Method::Post,
            path: path.to_owned(),
            ..Default::default()
        }),
    }
}

fn names(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::File(file) => file.name.clone(),
            Entry::Directory(folder) => folder.name.clone(),
        })
        .collect()
}

#[test]
fn imported_collections_keep_their_order_settings_and_survive_reload() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();

    let path = registry
        .import_collection(ImportedCollection {
            name: "Pet Store".into(),
            variables: HashMap::from([("base_url".into(), "https://pets.test".into())]),
            scripts: RequestScripts {
                pre_request: "pm.variables.set('a', 1)".into(),
                post_response: String::new(),
            },
            items: vec![
                ImportedItem::Folder {
                    name: "Pets".into(),
                    items: vec![
                        request("Update pet", "{{base_url}}/pets/1"),
                        request("Add pet", "{{base_url}}/pets"),
                    ],
                },
                request("Health", "{{base_url}}/health"),
            ],
        })
        .unwrap();

    assert_eq!(path, fixture.0.join("Pet Store"));
    assert_eq!(
        names(&registry.collections()[0].entries),
        ["Pets", "Health"]
    );

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = &reloaded.collections()[0];

    assert_eq!(
        collection.local_env().resolve("base_url"),
        Some("https://pets.test")
    );
    assert_eq!(collection.scripts().pre_request, "pm.variables.set('a', 1)");
    assert_eq!(names(&collection.entries), ["Pets", "Health"]);

    let Entry::Directory(folder) = &collection.entries[0] else {
        panic!("expected a folder");
    };
    assert_eq!(names(&folder.entries), ["Update pet", "Add pet"]);

    let Entry::File(file) = &folder.entries[0] else {
        panic!("expected a request");
    };
    let Request::Http(http) = &file.request;
    assert_eq!(http.method, Method::Post);
    assert_eq!(http.path, "{{base_url}}/pets/1");
}

#[test]
fn imported_names_become_safe_unique_file_names() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let imported = || ImportedCollection {
        name: "../Pets".into(),
        variables: HashMap::new(),
        scripts: RequestScripts::default(),
        items: vec![
            request("Get /pets/{id}", "/pets/{id}"),
            request("Get /pets/{id}", "/pets/{id}"),
            // Would otherwise replace the collection's environment file.
            request("environment", "/environment"),
            request(".", "/"),
            request(&"long ".repeat(100), "/long"),
        ],
    };

    let first = registry.import_collection(imported()).unwrap();
    let second = registry.import_collection(imported()).unwrap();

    assert_eq!(first, fixture.0.join("-Pets"));
    assert_eq!(second, fixture.0.join("-Pets 2"));

    let collection = &registry.collections()[0];
    let files: Vec<_> = collection
        .entries
        .iter()
        .map(|entry| {
            entry
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        files[..4],
        [
            "Get -pets-{id}.toml",
            "Get -pets-{id} 2.toml",
            "environment 2.toml",
            "Request.toml",
        ]
    );
    assert!(files[4].len() < 130);

    // Names shown in the sidebar are kept as imported.
    assert_eq!(
        names(&collection.entries)[..2],
        ["Get /pets/{id}", "Get /pets/{id}"]
    );
    assert!(CollectionRegistry::from_path(&fixture.0).is_ok());
}
