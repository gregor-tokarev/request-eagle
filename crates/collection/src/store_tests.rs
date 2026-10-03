use std::{fs, path::Path};

use crate::{CollectionRegistry, Collections, SavedLocation};

fn write_request(path: &Path, id: &str, name: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nschema_version = 1\n\n[request]\ntype = \"http\"\nmethod = \"GET\"\npath = \"/\"\n"
        ),
    )
    .unwrap();
}

/// `API` holds an empty `admin` folder, `list.toml` and `users/get.toml`.
fn collections(root: &Path) -> Collections {
    let api = root.join("API");
    fs::create_dir_all(api.join("admin")).unwrap();
    write_request(&api.join("list.toml"), "list", "List users");
    write_request(&api.join("users").join("get.toml"), "get", "Get user");

    Collections::new(CollectionRegistry::from_path(root))
}

#[test]
fn requests_in_a_branch_follow_the_sidebar_order_with_their_locations() {
    let root = tempfile::tempdir().unwrap();
    let collections = collections(root.path());
    let api = root.path().join("API");

    let requests = collections.requests_in(&api).unwrap();
    let locations: Vec<_> = requests.iter().map(|(location, _)| location).collect();
    assert_eq!(
        locations,
        [
            &SavedLocation {
                path: api.join("list.toml"),
                id: "list".into(),
                name: "List users".into(),
                collection: api.clone(),
            },
            &SavedLocation {
                path: api.join("users").join("get.toml"),
                id: "get".into(),
                name: "Get user".into(),
                collection: api.clone(),
            },
        ]
    );

    assert_eq!(
        collections.requests_in(&api.join("users")).unwrap().len(),
        1
    );
    assert!(
        collections
            .requests_in(&api.join("admin"))
            .unwrap()
            .is_empty()
    );
    assert!(collections.requests_in(&api.join("list.toml")).is_none());
}

#[test]
fn save_destinations_list_collections_and_folders_in_sidebar_order() {
    let root = tempfile::tempdir().unwrap();
    let collections = collections(root.path());
    let api = root.path().join("API");

    let destinations: Vec<_> = collections
        .save_destinations()
        .into_iter()
        .map(|destination| (destination.name(), destination.is_collection()))
        .collect();
    assert_eq!(
        destinations,
        [
            ("API".to_owned(), true),
            ("admin".to_owned(), false),
            ("users".to_owned(), false),
        ]
    );
    assert!(
        collections
            .save_destinations()
            .iter()
            .all(|destination| destination.collection == api)
    );
}

#[test]
fn a_location_names_its_collection_and_folders_from_its_path() {
    let location = SavedLocation {
        path: Path::new("/data/API/users/admins/get.toml").into(),
        id: "get".into(),
        name: "Get admin".into(),
        collection: Path::new("/data/API").into(),
    };

    assert_eq!(location.collection_name(), "API");
    assert_eq!(location.folders(), ["users", "admins"]);
    assert_eq!(
        location.environment_path(),
        Path::new("/data/API/environment.toml")
    );
}
