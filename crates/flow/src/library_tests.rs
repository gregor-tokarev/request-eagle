use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::{Block, BlockKind, Flow, FlowLibrary, FlowLibraryError};

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

fn greeting() -> Flow {
    let mut flow = Flow::starter();
    flow.blocks.push(Block {
        id: "b2".to_owned(),
        title: Some("Greeting".to_owned()),
        x: 200.,
        y: 40.,
        kind: BlockKind::String {
            value: "hello".to_owned(),
        },
    });
    flow
}

#[test]
fn a_missing_directory_has_no_flows() {
    let fixture = Fixture::new();
    let library = FlowLibrary::load(&fixture.0);

    assert!(library.flows().is_empty());
    assert!(library.skipped().is_empty());
}

#[test]
fn flows_are_saved_in_their_directory_and_survive_reload() {
    let fixture = Fixture::new();
    let mut library = FlowLibrary::load(&fixture.0);
    let path = library.create("Checkout", Flow::starter()).unwrap();

    assert_eq!(path, fixture.0.join("Checkout.toml"));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("name = \"Checkout\""), "{text}");
    assert!(text.contains("[[flow.blocks]]"), "{text}");

    let reloaded = FlowLibrary::load(&fixture.0);
    let saved = reloaded.get(&path).unwrap();
    assert_eq!(saved.name, "Checkout");
    assert!(Uuid::parse_str(&saved.id).is_ok());
    assert_eq!(saved.flow, Flow::starter());

    // An empty flow still has its table, so it loads.
    let empty = library.create("Checkout", Flow::default()).unwrap();
    assert_eq!(empty, fixture.0.join("Checkout 2.toml"));
    let reloaded = FlowLibrary::load(&fixture.0);
    assert_eq!(reloaded.get(&empty).unwrap().name, "Checkout 2");
    assert_eq!(reloaded.get(&empty).unwrap().flow, Flow::default());
}

#[test]
fn names_that_are_not_file_names_still_name_files() {
    let fixture = Fixture::new();
    let mut library = FlowLibrary::load(&fixture.0);

    let path = library.create("Orders / Refunds", Flow::starter()).unwrap();
    assert_eq!(path, fixture.0.join("Orders - Refunds.toml"));
    assert_eq!(library.get(&path).unwrap().name, "Orders / Refunds");

    let hidden = library.create("..", Flow::starter()).unwrap();
    assert_eq!(hidden, fixture.0.join("Flow.toml"));

    assert!(matches!(
        library.create("  ", Flow::starter()),
        Err(FlowLibraryError::InvalidName)
    ));
}

#[test]
fn flows_are_listed_by_name() {
    let fixture = Fixture::new();
    let mut library = FlowLibrary::load(&fixture.0);
    library.create("beta", Flow::starter()).unwrap();
    let alpha = library.create("Gamma", Flow::starter()).unwrap();
    library.create("Delta", Flow::starter()).unwrap();
    let id = library.get(&alpha).unwrap().id.clone();
    library.rename(&alpha, &id, "alpha").unwrap();

    let names = |library: &FlowLibrary| {
        library
            .flows()
            .iter()
            .map(|saved| saved.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&library), ["alpha", "beta", "Delta"]);
    assert_eq!(
        names(&FlowLibrary::load(&fixture.0)),
        ["alpha", "beta", "Delta"]
    );
}

#[test]
fn updates_and_renames_keep_the_file_unless_it_holds_another_flow() {
    let fixture = Fixture::new();
    let mut library = FlowLibrary::load(&fixture.0);
    let path = library.create("Sync", Flow::starter()).unwrap();
    let id = library.get(&path).unwrap().id.clone();

    library.update(&path, &id, greeting()).unwrap();
    library.rename(&path, &id, "Nightly sync").unwrap();

    let reloaded = FlowLibrary::load(&fixture.0);
    let saved = reloaded.get(&path).unwrap();
    assert_eq!(saved.name, "Nightly sync");
    assert_eq!(saved.flow, greeting());

    assert!(matches!(
        library.update(&path, "another", Flow::starter()),
        Err(FlowLibraryError::Replaced)
    ));
    assert!(matches!(
        library.rename(&path, &id, ""),
        Err(FlowLibraryError::InvalidName)
    ));

    // Another flow saved over the file is not replaced.
    let mut other = FlowLibrary::load(&fixture.0);
    other.delete(&path).unwrap();
    let replacement = other.create("Sync", Flow::default()).unwrap();
    assert_eq!(replacement, path);
    assert!(matches!(
        library.update(&path, &id, Flow::starter()),
        Err(FlowLibraryError::Replaced)
    ));
    assert_eq!(
        FlowLibrary::load(&fixture.0).get(&path).unwrap().flow,
        Flow::default()
    );
}

#[test]
fn duplicates_and_deletes() {
    let fixture = Fixture::new();
    let mut library = FlowLibrary::load(&fixture.0);
    let path = library.create("Sync", greeting()).unwrap();

    let copy = library.duplicate(&path).unwrap();
    assert_eq!(copy, fixture.0.join("Sync Copy.toml"));
    let copied = library.get(&copy).unwrap();
    assert_eq!(copied.name, "Sync Copy");
    assert_eq!(copied.flow, greeting());
    assert_ne!(copied.id, library.get(&path).unwrap().id);

    library.delete(&path).unwrap();
    assert!(!path.exists());
    assert!(library.get(&path).is_none());
    assert!(matches!(
        library.delete(&path),
        Err(FlowLibraryError::NotFound)
    ));
    assert_eq!(FlowLibrary::load(&fixture.0).flows().len(), 1);
}

#[test]
fn unreadable_files_are_skipped() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(fixture.0.join("Broken.toml"), "name = ").unwrap();
    fs::write(fixture.0.join("notes.txt"), "not a flow").unwrap();
    let mut library = FlowLibrary::load(&fixture.0);
    library.create("Works", Flow::starter()).unwrap();

    let reloaded = FlowLibrary::load(&fixture.0);
    assert_eq!(reloaded.flows().len(), 1);
    assert_eq!(reloaded.skipped().len(), 1);
    assert_eq!(reloaded.skipped()[0].path, fixture.0.join("Broken.toml"));
}

/// A flow file as Request Eagle 0.1.22 saved it in a collection.
fn legacy_flow(name: &str) -> String {
    format!("id = \"{name}\"\nname = \"{name}\"\nschema_version = 1\n\n[flow]\nblocks = []\n")
}

fn names(library: &FlowLibrary) -> Vec<String> {
    let mut names: Vec<_> = library
        .flows()
        .iter()
        .map(|saved| saved.name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn flows_saved_in_collections_move_to_the_flows_directory_once() {
    let fixture = Fixture::new();
    let collections = fixture.0.join("collections");
    let flows = fixture.0.join("flows");
    let collection = collections.join("API");
    let folder = collection.join("Orders");
    fs::create_dir_all(&folder).unwrap();
    let request = "id = \"r1\"\nname = \"List\"\nschema_version = 1\n\n[request]\ntype = \"http\"\nmethod = \"GET\"\npath = \"/\"\n";
    fs::write(collection.join("List.toml"), request).unwrap();
    fs::write(collection.join("Checkout.toml"), legacy_flow("Checkout")).unwrap();
    fs::write(folder.join("Checkout.toml"), legacy_flow("Nested")).unwrap();
    fs::write(folder.join("Broken.toml"), "flow = ").unwrap();
    // A request stays a request, even with a `flow` table.
    let annotated = format!("{request}\n[flow]\nblocks = []\n");
    fs::write(folder.join("Annotated.toml"), annotated).unwrap();
    // A variable named like the table does not make an environment a flow.
    fs::write(collection.join("environment.toml"), "flow = \"checkout\"\n").unwrap();

    let mut moved = crate::move_flows_out_of_collections(&collections, &flows).unwrap();
    moved.sort();
    assert_eq!(
        moved,
        [flows.join("Checkout 2.toml"), flows.join("Checkout.toml")]
    );
    assert!(collection.join("List.toml").exists());
    assert!(collection.join("environment.toml").exists());
    assert!(folder.join("Broken.toml").exists());
    assert!(folder.join("Annotated.toml").exists());
    assert!(!folder.join("Checkout.toml").exists());
    assert_eq!(names(&FlowLibrary::load(&flows)), ["Checkout", "Nested"]);

    // Once the flows directory exists, collections are not read again.
    fs::write(collection.join("Later.toml"), legacy_flow("Later")).unwrap();
    assert!(
        crate::move_flows_out_of_collections(&collections, &flows)
            .unwrap()
            .is_empty()
    );
    assert!(collection.join("Later.toml").exists());
}

#[cfg(unix)]
#[test]
fn flows_move_out_of_collections_linked_into_the_collections_directory() {
    let fixture = Fixture::new();
    let collections = fixture.0.join("collections");
    let flows = fixture.0.join("flows");
    let linked = fixture.0.join("elsewhere").join("Linked");
    fs::create_dir_all(&linked).unwrap();
    fs::create_dir_all(&collections).unwrap();
    fs::write(linked.join("Sync.toml"), legacy_flow("Sync")).unwrap();
    std::os::unix::fs::symlink(&linked, collections.join("Linked")).unwrap();

    let moved = crate::move_flows_out_of_collections(&collections, &flows).unwrap();

    assert_eq!(moved, [flows.join("Sync.toml")]);
    assert!(!linked.join("Sync.toml").exists());
    assert_eq!(names(&FlowLibrary::load(&flows)), ["Sync"]);
}

#[test]
fn a_flow_moved_by_another_start_meanwhile_keeps_its_one_copy() {
    let fixture = Fixture::new();
    let flows = fixture.0.join("flows");
    fs::create_dir_all(&flows).unwrap();
    let source = legacy_flow("Checkout");
    // Another start already moved the flow: its copy is here and the file
    // it came from is gone.
    fs::write(flows.join("Checkout.toml"), &source).unwrap();
    let gone = fixture
        .0
        .join("collections")
        .join("API")
        .join("Checkout.toml");

    let result = crate::library::move_flow(&gone, &source, &flows, "Checkout");

    assert!(result.is_err());
    let files: Vec<_> = fs::read_dir(&flows)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, ["Checkout.toml"]);
    assert_eq!(
        fs::read_to_string(flows.join("Checkout.toml")).unwrap(),
        source
    );
}
