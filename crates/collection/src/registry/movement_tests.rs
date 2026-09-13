use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{CollectionEditError, CollectionRegistry, Entry, MovePlacement};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("request-eagle-move-{}", Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn paths(registry: &mut CollectionRegistry, parent: &Path) -> Vec<PathBuf> {
    registry
        .entries_mut(parent)
        .unwrap()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

#[test]
fn reordered_requests_and_folders_survive_reload_rename_creation_and_deletion() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let first = registry.create_request(&collection).unwrap();
    let folder = registry.create_folder(&collection).unwrap();
    let last = registry.create_request(&collection).unwrap();
    registry
        .move_entry(&last, &first, MovePlacement::Before)
        .unwrap();
    registry
        .move_entry(&folder, &last, MovePlacement::After)
        .unwrap();
    assert_eq!(
        paths(&mut registry, &collection),
        [last.clone(), folder.clone(), first.clone()]
    );
    registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(
        paths(&mut registry, &collection),
        [last.clone(), folder.clone(), first.clone()]
    );

    let renamed = registry.rename(&folder, "Renamed").unwrap();
    let added = registry.create_request(&collection).unwrap();
    registry.delete(&first).unwrap();
    registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(paths(&mut registry, &collection), [last, renamed, added]);
}

#[test]
fn moves_requests_between_folders_and_collections_without_changing_their_contents() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let first = registry.create_collection().unwrap();
    let second = registry.create_collection().unwrap();
    let folder = registry.create_folder(&first).unwrap();
    let request = registry.create_request(&folder).unwrap();
    let bytes = fs::read(&request).unwrap();
    let moved = registry
        .move_entry(&request, &second, MovePlacement::Inside)
        .unwrap();
    assert!(!request.exists());
    assert_eq!(fs::read(&moved).unwrap(), bytes);
    assert!(paths(&mut registry, &folder).is_empty());
    let back = registry
        .move_entry(&moved, &folder, MovePlacement::Inside)
        .unwrap();
    assert_eq!(back, request);
    registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert!(paths(&mut registry, &second).is_empty());
    assert_eq!(paths(&mut registry, &folder), [request]);
}

#[test]
fn moving_a_folder_rebases_descendants_and_preserves_their_order() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let first = registry.create_collection().unwrap();
    let second = registry.create_collection().unwrap();
    let folder = registry.create_folder(&first).unwrap();
    let nested = registry.create_folder(&folder).unwrap();
    let request = registry.create_request(&nested).unwrap();
    let next = registry.create_request(&nested).unwrap();
    registry
        .move_entry(&next, &request, MovePlacement::Before)
        .unwrap();
    let moved = registry
        .move_entry(&folder, &second, MovePlacement::Inside)
        .unwrap();
    let new_nested = moved.join(nested.file_name().unwrap());
    let new_request = new_nested.join(request.file_name().unwrap());
    let new_next = new_nested.join(next.file_name().unwrap());
    assert!(!folder.exists());
    assert!(new_request.is_file());
    assert_eq!(
        paths(&mut registry, &new_nested),
        [new_next.clone(), new_request.clone()]
    );
    let Entry::File(file) = &registry.entries_mut(&new_nested).unwrap()[1] else {
        panic!()
    };
    assert_eq!(file.path, new_request);
    registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(paths(&mut registry, &new_nested), [new_next, new_request]);
    assert!(paths(&mut registry, &first).is_empty());
}

#[test]
fn invalid_moves_and_order_write_errors_leave_files_and_model_unchanged() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let first = registry.create_collection().unwrap();
    let second = registry.create_collection().unwrap();
    let folder = registry.create_folder(&first).unwrap();
    let nested = registry.create_folder(&folder).unwrap();
    let request = registry.create_request(&folder).unwrap();
    let collision = registry.create_request(&second).unwrap();
    let original = fs::read(&collision).unwrap();
    assert!(matches!(
        registry.move_entry(&folder, &nested, MovePlacement::Inside),
        Err(CollectionEditError::InvalidMove)
    ));
    assert!(matches!(
        registry.move_entry(&folder, &folder, MovePlacement::Inside),
        Err(CollectionEditError::InvalidMove)
    ));
    assert!(matches!(
        registry.move_entry(&request, &second, MovePlacement::Inside),
        Err(CollectionEditError::AlreadyExists)
    ));
    assert!(
        registry
            .move_entry(&first, &second, MovePlacement::Inside)
            .is_err()
    );
    assert_eq!(fs::read(&collision).unwrap(), original);
    assert_eq!(
        paths(&mut registry, &folder),
        [nested.clone(), request.clone()]
    );

    fs::remove_file(second.join(".request-eagle-order.json")).unwrap();
    fs::create_dir(second.join(".request-eagle-order.json")).unwrap();
    assert!(
        registry
            .move_entry(&nested, &second, MovePlacement::Inside)
            .is_err()
    );
    assert!(nested.is_dir());
    assert!(!second.join(nested.file_name().unwrap()).exists());
    assert_eq!(paths(&mut registry, &folder), [nested, request]);
}
