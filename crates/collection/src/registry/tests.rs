use std::time::SystemTime;

use super::CollectionRegistry;
use std::{fs, path::PathBuf};

struct RelativeFixture {
    relative: PathBuf,
    absolute: PathBuf,
}

impl RelativeFixture {
    fn new() -> Self {
        let relative = PathBuf::from(test_directory().file_name().unwrap());
        let absolute = std::env::current_dir().unwrap().join(&relative);

        Self { relative, absolute }
    }
}

impl Drop for RelativeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.absolute);
    }
}

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

    assert_eq!(registry.len(), 2);
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

    assert!(registry.is_empty());
}

#[test]
fn missing_relative_root_is_absolute_without_creating_directories() {
    let fixture = RelativeFixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.relative).unwrap();

    assert!(registry.is_empty());
    assert_eq!(registry.directory.as_ref(), Some(&fixture.absolute));
    assert!(!fixture.absolute.exists());

    let collection = registry.create_collection().unwrap();
    let request = registry.create_request(&collection).unwrap();

    assert_eq!(collection, fixture.absolute.join("New Collection"));
    assert!(request.is_absolute());
    assert!(registry.collections()[0].local_env().path.is_absolute());
}

#[test]
fn relative_and_absolute_roots_keep_request_identity_for_reloading_and_saving() {
    let fixture = RelativeFixture::new();
    let mut seed = CollectionRegistry::from_path(&fixture.absolute).unwrap();
    let collection = seed.create_collection().unwrap();
    let folder = seed.create_folder(&collection).unwrap();
    let path = seed.create_request(&folder).unwrap();
    let environment_path = collection.join("environment.toml");
    fs::write(&environment_path, "base_url = 'https://example.test'\n").unwrap();

    let relative = CollectionRegistry::from_path(&fixture.relative).unwrap();
    let file = relative
        .file(&path)
        .expect("relative loading must retain absolute request paths");
    let id = file.id.clone();

    assert_eq!(relative.collections()[0].path, collection);
    assert_eq!(relative.collections()[0].local_env().path, environment_path);
    assert_eq!(
        relative.collections()[0].local_env().resolve("base_url"),
        Some("https://example.test")
    );

    let mut absolute = CollectionRegistry::from_path(&fixture.absolute).unwrap();
    let edited = crate::HttpRequest {
        path: "{{base_url}}/saved-after-relaunch".into(),
        ..Default::default()
    };
    absolute
        .update_request(&file.path, &id, edited.into())
        .unwrap();

    let reloaded = CollectionRegistry::from_path(&fixture.relative).unwrap();
    let saved = reloaded.file(&path).unwrap();
    let crate::Request::Http(request) = &saved.request;

    assert_eq!(saved.id, id);
    assert_eq!(request.path, "{{base_url}}/saved-after-relaunch");
}

#[test]
fn parent_directory_roots_share_the_resolved_absolute_identity() {
    let fixture = RelativeFixture::new();
    fs::create_dir_all(fixture.absolute.join("nested")).unwrap();
    let mut seed = CollectionRegistry::from_path(&fixture.absolute).unwrap();
    let collection = seed.create_collection().unwrap();
    let path = seed.create_request(&collection).unwrap();
    let id = seed.file(&path).unwrap().id.clone();
    let parent_root = fixture.relative.join("nested/..");
    let parent_registry = CollectionRegistry::from_path(parent_root).unwrap();
    let resolved_root = fixture.absolute.canonicalize().unwrap();
    let resolved_registry = CollectionRegistry::from_path(&resolved_root).unwrap();
    let expected_path = resolved_root.join(path.strip_prefix(&fixture.absolute).unwrap());

    assert_eq!(parent_registry.directory.as_ref(), Some(&resolved_root));
    assert_eq!(parent_registry.file(&expected_path).unwrap().id, id);
    assert_eq!(resolved_registry.file(&expected_path).unwrap().id, id);
}

#[cfg(unix)]
#[test]
fn named_symlink_roots_keep_their_absolute_named_location() {
    use std::os::unix::fs::symlink;

    let fixture = RelativeFixture::new();
    let target = fixture.absolute.join("target");
    let alias = fixture.absolute.join("alias");
    fs::create_dir_all(&target).unwrap();
    symlink(&target, &alias).unwrap();
    let mut registry = CollectionRegistry::from_path(fixture.relative.join("alias")).unwrap();
    let collection = registry.create_collection().unwrap();
    let request = registry.create_request(&collection).unwrap();

    assert_eq!(collection, alias.join("New Collection"));
    assert!(request.starts_with(&alias));
    assert!(
        registry.collections()[0]
            .local_env()
            .path
            .starts_with(&alias)
    );
    assert!(
        CollectionRegistry::from_path(&alias)
            .unwrap()
            .file(&request)
            .is_some()
    );
}
