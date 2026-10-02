use std::{fs, path::PathBuf};

use flow::{BlockKind, Flow};
use uuid::Uuid;

use crate::{CollectionEditError, CollectionRegistry, Entry, MovePlacement};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("request-eagle-flows-{}", Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn flows_are_saved_next_to_requests_and_survive_reload() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let request = registry.create_request(&collection).unwrap();
    let path = registry
        .create_flow(&collection, "Checkout", Flow::starter())
        .unwrap();

    assert_eq!(path, collection.join("Checkout.toml"));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("[[flow.blocks]]"), "{text}");
    assert!(text.contains("type = \"start\""), "{text}");

    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let entries = &reloaded.collections()[0].entries;
    assert!(matches!(&entries[0], Entry::File(file) if file.path == request));
    let Entry::Flow(flow) = &entries[1] else {
        panic!("expected the flow after the request");
    };
    assert_eq!(flow.name, "Checkout");
    assert!(Uuid::parse_str(&flow.id).is_ok());
    assert_eq!(flow.flow, Flow::starter());
    assert_eq!(reloaded.flow(&path).unwrap().id, flow.id);
    assert!(reloaded.file(&path).is_none());
    assert!(reloaded.flow(&request).is_none());

    // An empty flow still has its table, so it loads as a flow.
    let empty = registry
        .create_flow(&collection, "Checkout", Flow::default())
        .unwrap();
    assert_eq!(empty, collection.join("Checkout 2.toml"));
    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    assert_eq!(reloaded.flow(&empty).unwrap().flow, Flow::default());
}

#[test]
fn flows_update_rename_move_and_delete_like_requests() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let folder = registry.create_folder(&collection).unwrap();
    let path = registry
        .create_flow(&collection, "Sync", Flow::starter())
        .unwrap();
    let id = registry.flow(&path).unwrap().id.clone();

    let mut flow = Flow::starter();
    flow.blocks.push(flow::Block {
        id: "b2".to_owned(),
        title: Some("Greeting".to_owned()),
        x: 200.,
        y: 40.,
        kind: BlockKind::String {
            value: "hello".to_owned(),
        },
    });
    registry.update_flow(&path, &id, flow.clone()).unwrap();
    assert_eq!(registry.flow(&path).unwrap().flow, flow);
    assert!(matches!(
        registry.update_flow(&path, "another", Flow::default()),
        Err(CollectionEditError::FlowReplaced)
    ));

    assert_eq!(registry.rename(&path, "Nightly sync").unwrap(), path);
    assert_eq!(registry.flow(&path).unwrap().name, "Nightly sync");

    let moved = registry
        .move_entry(&path, &folder, MovePlacement::Inside)
        .unwrap();
    assert_eq!(moved, folder.join("Sync.toml"));
    let reloaded = CollectionRegistry::from_path(&fixture.0).unwrap();
    let saved = reloaded.flow(&moved).unwrap();
    assert_eq!(saved.name, "Nightly sync");
    assert_eq!(saved.id, id);
    assert_eq!(saved.flow, flow);

    registry.delete(&moved).unwrap();
    assert!(!moved.exists());
    assert!(registry.flow(&moved).is_none());
}

#[test]
fn finds_requests_by_id_wherever_they_moved() {
    let fixture = Fixture::new();
    let mut registry = CollectionRegistry::from_path(&fixture.0).unwrap();
    let collection = registry.create_collection().unwrap();
    let folder = registry.create_folder(&collection).unwrap();
    let path = registry.create_request(&collection).unwrap();
    let id = registry.file(&path).unwrap().id.clone();

    let moved = registry
        .move_entry(&path, &folder, MovePlacement::Inside)
        .unwrap();
    let (found_collection, file) = registry.request_by_id(&id).unwrap();

    assert_eq!(found_collection.path, collection);
    assert_eq!(file.path, moved);
    assert!(registry.request_by_id("missing").is_none());
}

#[test]
fn a_file_with_neither_a_request_nor_a_flow_does_not_load() {
    let fixture = Fixture::new();
    let collection = fixture.0.join("API");
    fs::create_dir_all(&collection).unwrap();
    fs::write(
        collection.join("Odd.toml"),
        "id = \"x\"\nname = \"Odd\"\nschema_version = 1\n",
    )
    .unwrap();

    let error = CollectionRegistry::from_path(&fixture.0)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("API"), "{error}");
}
