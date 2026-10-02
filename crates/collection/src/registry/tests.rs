use std::time::SystemTime;

use super::CollectionRegistry;
use std::{fs, path::PathBuf};

fn test_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "request-eagle-collection-registry-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn loads_collection_directories_in_name_order() {
    let root = test_directory();
    let alpha = root.join("alpha");
    let beta = root.join("beta");

    fs::create_dir_all(&alpha).unwrap();
    fs::create_dir_all(&beta).unwrap();
    fs::write(alpha.join("environment.toml"), "base_url = \"local\"\n").unwrap();
    fs::write(root.join("not-a-collection.toml"), "ignored = true\n").unwrap();

    let registry = CollectionRegistry::from_path(&root);

    assert_eq!(registry.collections().len(), 2);
    assert_eq!(registry.collections()[0].path, alpha);
    assert_eq!(registry.collections()[1].path, beta);
    assert_eq!(
        registry.collections()[0].local_env().resolve("base_url"),
        Some("local")
    );
    assert!(registry.collections()[1].local_env().entries.is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_collections_directory_loads_an_empty_registry() {
    let root = test_directory();

    let registry = CollectionRegistry::from_path(root);

    assert!(registry.collections().is_empty());
}

#[test]
fn unreadable_files_are_left_out_and_untouched() {
    let root = test_directory();
    let api = root.join("API");
    let billing = root.join("Billing");
    let shipping = root.join("Shipping");
    let request = "id = 'one'\nname = 'Health'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/health'\n";
    let conflicted = "<<<<<<< HEAD\nid = 'two'\n=======\nid = 'three'\n>>>>>>> branch\n";

    fs::create_dir_all(api.join("Users")).unwrap();
    fs::create_dir_all(&billing).unwrap();
    fs::create_dir_all(&shipping).unwrap();
    fs::write(api.join("Health.toml"), request).unwrap();
    fs::write(api.join("Users/Broken.toml"), conflicted).unwrap();
    fs::write(api.join(".request-eagle-order.json"), "[\"Health.toml\",").unwrap();
    fs::write(billing.join("Invoices.toml"), request).unwrap();
    // A collection whose variables cannot be read is left out whole, so
    // saving them cannot replace the file.
    fs::write(billing.join("environment.toml"), conflicted).unwrap();
    fs::write(shipping.join(".request-eagle-collection.toml"), conflicted).unwrap();

    let mut registry = CollectionRegistry::from_path(&root);

    let [collection] = registry.collections() else {
        panic!("expected only the API collection");
    };
    assert_eq!(collection.path, api);
    assert!(registry.file(&api.join("Health.toml")).is_some());
    let skipped = registry
        .skipped()
        .iter()
        .map(|skipped| skipped.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        skipped,
        [
            api.join("Users/Broken.toml"),
            api.join(".request-eagle-order.json"),
            billing.join("environment.toml"),
            shipping.join(".request-eagle-collection.toml"),
        ]
    );
    assert!(registry.skipped()[0].error.contains("line 1"));

    // A new request with the same name does not replace the broken file.
    let created = registry
        .create_request_with(
            &api.join("Users"),
            "Broken",
            request::HttpRequest::default().into(),
        )
        .unwrap();
    assert_eq!(created, api.join("Users/Broken 2.toml"));
    assert_eq!(
        fs::read_to_string(api.join("Users/Broken.toml")).unwrap(),
        conflicted
    );
    assert_eq!(
        fs::read_to_string(billing.join("environment.toml")).unwrap(),
        conflicted
    );

    // Nor does saving the order of a new item replace an unreadable order.
    registry
        .create_request_with(&api, "Status", request::HttpRequest::default().into())
        .unwrap();
    assert_eq!(
        fs::read_to_string(api.join(".request-eagle-order.json")).unwrap(),
        "[\"Health.toml\","
    );

    // Skipped files go with their deleted folder.
    registry.delete(&api.join("Users")).unwrap();
    assert_eq!(
        registry.skipped()[0].path,
        api.join(".request-eagle-order.json")
    );

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn an_order_that_cannot_be_read_does_not_block_new_items() {
    use std::os::unix::fs::PermissionsExt;

    let root = test_directory();
    let api = root.join("API");
    let order = api.join(".request-eagle-order.json");
    fs::create_dir_all(&api).unwrap();
    fs::write(&order, "[]").unwrap();
    fs::set_permissions(&order, fs::Permissions::from_mode(0o200)).unwrap();

    // Privileged users read the file anyway.
    if fs::read(&order).is_err() {
        let mut registry = CollectionRegistry::from_path(&root);
        assert_eq!(registry.skipped()[0].path, order);

        registry
            .create_request_with(&api, "Health", request::HttpRequest::default().into())
            .unwrap();
        let folder = registry.create_folder(&api).unwrap();
        registry.rename(&folder, "Users").unwrap();
        assert!(api.join("Health.toml").is_file());
        assert!(api.join("Users").is_dir());

        fs::set_permissions(&order, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read_to_string(&order).unwrap(), "[]");
    }

    fs::remove_dir_all(root).unwrap();
}
