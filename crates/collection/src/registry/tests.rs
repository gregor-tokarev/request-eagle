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

#[test]
fn interior_parent_components_share_request_identity_with_the_ordinary_root() {
    let fixture = RelativeFixture::new();
    fs::create_dir_all(fixture.absolute.join("nested")).unwrap();
    let root = fixture.absolute.join("collections");
    let mut seed = CollectionRegistry::from_path(&root).unwrap();
    let collection = seed.create_collection().unwrap();
    let path = seed.create_request(&collection).unwrap();
    let id = seed.file(&path).unwrap().id.clone();
    let traversed = fixture.relative.join("nested/../collections");
    let mut registry = CollectionRegistry::from_path(&traversed).unwrap();

    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert_eq!(registry.file(&path).unwrap().id, id);
    assert_eq!(
        registry.collections()[0].local_env().path,
        collection.join("environment.toml")
    );

    let edited = crate::HttpRequest {
        path: "https://example.test/saved-after-relaunch".into(),
        ..Default::default()
    };
    registry.update_request(&path, &id, edited.into()).unwrap();
    let ordinary = CollectionRegistry::from_path(root).unwrap();
    let saved = ordinary.file(&path).unwrap();
    let crate::Request::Http(request) = &saved.request;

    assert_eq!(saved.id, id);
    assert_eq!(request.path, "https://example.test/saved-after-relaunch");
}

#[test]
fn interior_parent_components_allow_a_missing_named_suffix_without_creation() {
    let fixture = RelativeFixture::new();
    fs::create_dir_all(fixture.absolute.join("nested")).unwrap();
    let root = fixture.absolute.join("new/collections");
    let registry =
        CollectionRegistry::from_path(fixture.relative.join("nested/../new/collections")).unwrap();

    assert!(registry.is_empty());
    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert!(!fixture.absolute.join("new").exists());
}

#[cfg(unix)]
#[test]
fn missing_traversed_components_do_not_redirect_to_an_existing_registry() {
    let fixture = RelativeFixture::new();
    let root = fixture.absolute.join("collections");
    let mut ordinary = CollectionRegistry::from_path(&root).unwrap();
    ordinary.create_collection().unwrap();
    let traversed = fixture.relative.join("missing/../collections");
    let registry = CollectionRegistry::from_path(&traversed).unwrap();

    assert!(registry.is_empty());
    assert_eq!(
        registry.directory.as_ref(),
        Some(&std::path::absolute(traversed).unwrap())
    );
    assert!(!fixture.absolute.join("missing").exists());
    assert_eq!(CollectionRegistry::from_path(root).unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn file_components_cannot_be_traversed_as_parent_directories() {
    let fixture = RelativeFixture::new();
    fs::create_dir_all(&fixture.absolute).unwrap();
    fs::write(fixture.absolute.join("file"), "preserve this file").unwrap();
    let result = CollectionRegistry::from_path(fixture.relative.join("file/../collections"));

    assert!(
        matches!(result, Err(super::CollectionRegistryLoadError::Read { source, .. })
        if source.kind() == std::io::ErrorKind::NotADirectory)
    );
    assert!(!fixture.absolute.join("collections").exists());
}

#[test]
fn creating_a_missing_parent_traversal_root_keeps_paths_stable_on_reload() {
    let fixture = RelativeFixture::new();
    let traversed = fixture.relative.join("missing/../collections");
    let root = fixture.absolute.join("collections");
    let mut registry = CollectionRegistry::from_path(&traversed).unwrap();
    let collection = registry.create_collection().unwrap();
    let path = registry.create_request(&collection).unwrap();
    let id = registry.file(&path).unwrap().id.clone();

    assert_eq!(collection, root.join("New Collection"));
    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert_eq!(
        registry.collections()[0].local_env().path,
        collection.join("environment.toml")
    );
    assert_eq!(
        CollectionRegistry::from_path(&traversed)
            .unwrap()
            .file(&path)
            .unwrap()
            .id,
        id
    );
    assert_eq!(
        CollectionRegistry::from_path(&root)
            .unwrap()
            .file(&path)
            .unwrap()
            .id,
        id
    );
}

#[test]
fn importing_into_a_missing_parent_traversal_root_keeps_paths_stable_on_reload() {
    let fixture = RelativeFixture::new();
    let traversed = fixture.relative.join("missing/../collections");
    let root = fixture.absolute.join("collections");
    let mut registry = CollectionRegistry::from_path(&traversed).unwrap();
    let imported = registry
        .import_requests(
            "API",
            vec![crate::ImportedRequest {
                name: "Fetch".into(),
                folders: vec!["Users".into()],
                request: crate::HttpRequest::default(),
            }],
        )
        .unwrap();
    let file = &imported[0];

    assert_eq!(file.collection_path, root.join("API"));
    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert_eq!(
        registry.collections()[0].local_env().path,
        file.collection_path.join("environment.toml")
    );
    assert_eq!(
        CollectionRegistry::from_path(&traversed)
            .unwrap()
            .file(&file.path)
            .unwrap()
            .id,
        file.id
    );
    assert_eq!(
        CollectionRegistry::from_path(&root)
            .unwrap()
            .file(&file.path)
            .unwrap()
            .id,
        file.id
    );
}

#[cfg(unix)]
#[test]
fn parent_normalization_does_not_bypass_directory_search_permissions() {
    use std::{io, os::unix::fs::PermissionsExt};

    let fixture = RelativeFixture::new();
    let blocked = fixture.absolute.join("blocked");
    fs::create_dir_all(&blocked).unwrap();
    fs::create_dir(fixture.absolute.join("collections")).unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let traversed = fixture.relative.join("blocked/../collections");
    let denied = fs::read_dir(&traversed)
        .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
    let result = CollectionRegistry::from_path(&traversed);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();

    if !denied {
        eprintln!("Skipping search-permission assertion: this account can traverse mode 0000.");
        return;
    }

    assert!(
        matches!(result, Err(super::CollectionRegistryLoadError::Read { source, .. })
        if source.kind() == io::ErrorKind::PermissionDenied)
    );
}

#[cfg(unix)]
#[test]
fn interior_parent_components_preserve_untouched_symlink_ancestors() {
    use std::os::unix::fs::symlink;

    let fixture = RelativeFixture::new();
    let target = fixture.absolute.join("target");
    let alias = fixture.absolute.join("alias");
    fs::create_dir_all(target.join("nested")).unwrap();
    symlink(&target, &alias).unwrap();
    let root = alias.join("collections");
    let mut ordinary = CollectionRegistry::from_path(&root).unwrap();
    let collection = ordinary.create_collection().unwrap();
    let path = ordinary.create_request(&collection).unwrap();
    let registry =
        CollectionRegistry::from_path(fixture.relative.join("alias/nested/../collections"))
            .unwrap();

    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert_eq!(
        registry.file(&path).unwrap().id,
        ordinary.file(&path).unwrap().id
    );
    assert!(
        registry.collections()[0]
            .local_env()
            .path
            .starts_with(&alias)
    );
}

#[cfg(unix)]
#[test]
fn parent_traversal_after_a_symlink_follows_the_target_parent() {
    use std::os::unix::fs::symlink;

    let fixture = RelativeFixture::new();
    let target = fixture.absolute.join("target");
    fs::create_dir_all(target.join("nested")).unwrap();
    symlink(target.join("nested"), fixture.absolute.join("jump")).unwrap();
    let root = target.canonicalize().unwrap().join("collections");
    let mut ordinary = CollectionRegistry::from_path(&root).unwrap();
    let collection = ordinary.create_collection().unwrap();
    let path = ordinary.create_request(&collection).unwrap();
    let registry =
        CollectionRegistry::from_path(fixture.relative.join("jump/../collections")).unwrap();

    assert_eq!(registry.directory.as_ref(), Some(&root));
    assert_eq!(
        registry.file(&path).unwrap().id,
        ordinary.file(&path).unwrap().id
    );
    assert!(!fixture.absolute.join("collections").exists());
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
