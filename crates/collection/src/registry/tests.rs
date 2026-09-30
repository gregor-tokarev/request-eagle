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

    let registry = CollectionRegistry::from_path(&root).unwrap();

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

    let registry = CollectionRegistry::from_path(root).unwrap();

    assert!(registry.collections().is_empty());
}
