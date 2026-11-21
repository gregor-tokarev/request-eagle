use std::{collections::HashSet, fs, path::PathBuf};

use collection::CollectionRegistry;

use super::{
    delete_warning::{
        AffectedFlow, ConfirmationCheck, FlowImpact, TargetSnapshot, delete_warning_copy,
        recheck_confirmation,
    },
    tree::{CollectionTree, ItemKind},
};

pub(super) fn collections() -> CollectionRegistry {
    let directory = tempfile::tempdir().unwrap();
    let directory = directory.path();

    for name in ["Example API", "Status API"] {
        let collection = directory.join(name);
        fs::create_dir_all(&collection).unwrap();
        fs::write(
            collection.join("environment.toml"),
            "base_url = \"https://example.com\"\n",
        )
        .unwrap();
    }

    let mut requests: Vec<_> = (0..24)
        .map(|index| {
            (
                format!("Example API/Posts/request-{index:02}.toml"),
                format!("Get post {index}"),
                "GET",
                format!("/posts/{index}"),
            )
        })
        .collect();
    requests.extend([
        (
            "Example API/Posts/Comments/create.toml".into(),
            "Create comment".into(),
            "POST",
            "/comments".into(),
        ),
        (
            "Status API/Responses/not-found.toml".into(),
            "Not found".into(),
            "GET",
            "/status/404".into(),
        ),
    ]);

    for (index, (file, name, method, path)) in requests.into_iter().enumerate() {
        let file = directory.join(file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, format!(
            "id = \"request-{index}\"\nname = \"{name}\"\nschema_version = 1\n\n[request]\ntype = \"http\"\nmethod = \"{method}\"\npath = \"{path}\"\n"
        )).unwrap();
    }

    CollectionRegistry::from_path(directory)
}

#[test]
fn tree_preserves_hierarchy_and_filters_collapsed_collections() {
    let collections = collections();
    let tree = CollectionTree::new(&collections);

    assert_eq!(tree.roots.len(), 2);
    assert_eq!(
        tree.roots
            .iter()
            .map(|&index| tree.items[index].request_count)
            .sum::<usize>(),
        26
    );
    assert_eq!(
        tree.items
            .iter()
            .filter(|item| item.kind == ItemKind::Request("POST"))
            .count(),
        1
    );
    assert!(
        tree.items
            .iter()
            .any(|item| item.depth == 3 && item.kind == ItemKind::Request("POST"))
    );
    assert!(
        !tree
            .items
            .iter()
            .any(|item| item.label == "environment.toml")
    );

    let comment = tree
        .items
        .iter()
        .position(|item| item.kind == ItemKind::Request("POST"))
        .unwrap();
    assert_eq!(
        tree.location(comment),
        (
            "Example API".into(),
            vec!["Posts".into(), "Comments".into()]
        )
    );
    assert_eq!(tree.location(tree.roots[1]), ("Status API".into(), vec![]));

    for collection in collections.collections() {
        assert!(
            collection
                .local_env()
                .resolve("base_url")
                .unwrap()
                .starts_with("https://")
        );
    }

    let mut collapsed: HashSet<_> = tree.roots.iter().copied().collect();
    assert_eq!(tree.visible_rows(&collapsed, ""), tree.roots);

    let rows = tree.visible_rows(&collapsed, "comments");
    let labels: Vec<_> = rows
        .iter()
        .map(|&index| tree.items[index].label.as_ref())
        .collect();
    assert_eq!(
        labels,
        ["Example API", "Posts", "Comments", "Create comment"]
    );

    let rows = tree.visible_rows(&collapsed, "/status/404");
    assert_eq!(tree.items[*rows.last().unwrap()].label, "Not found");
    assert!(tree.visible_rows(&collapsed, "no-such-request").is_empty());

    collapsed.clear();
    assert_eq!(tree.visible_rows(&collapsed, "").len(), tree.items.len());
}

#[test]
fn empty_collections_and_folders_hold_a_placeholder_row() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("Empty API");
    fs::create_dir_all(collection.join("Drafts")).unwrap();
    let collections = CollectionRegistry::from_path(directory.path());
    let tree = CollectionTree::new(&collections);

    let kinds: Vec<_> = tree.items.iter().map(|item| item.kind).collect();
    assert_eq!(
        kinds,
        [ItemKind::Collection, ItemKind::Folder, ItemKind::Empty]
    );

    // The collection holds its folder, so only the folder is empty.
    let placeholder = &tree.items[2];
    assert_eq!(placeholder.parent, Some(1));
    assert_eq!(placeholder.depth, 2);
    assert_eq!(placeholder.path, collection.join("Drafts"));
    assert_eq!(tree.items[1].end, 3);

    // The placeholder shares its folder's path; looking it up finds the folder.
    assert_eq!(tree.index_of(&collection.join("Drafts")), Some(1));

    // Collapsing the folder hides it. It has no text to match, but shows
    // with a folder that matches, like the folder's requests would.
    let collapsed = HashSet::from([1]);
    assert_eq!(tree.visible_rows(&collapsed, ""), [0, 1]);
    assert_eq!(tree.visible_rows(&HashSet::new(), "drafts"), [0, 1, 2]);
    assert!(
        tree.visible_rows(&HashSet::new(), "empty api drafts")
            .is_empty()
    );
}

#[test]
fn collapsed_paths_find_their_collections_and_folders() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("Empty API");
    fs::create_dir_all(collection.join("Drafts")).unwrap();
    let collections = CollectionRegistry::from_path(directory.path());
    let tree = CollectionTree::new(&collections);

    // The folder's placeholder shares its path but is not a branch, and a
    // deleted folder is left out.
    let paths = HashSet::from([
        collection.clone(),
        collection.join("Drafts"),
        collection.join("Deleted"),
    ]);
    assert_eq!(tree.branches_at(&paths), HashSet::from([0, 1]));
    assert!(tree.branches_at(&HashSet::new()).is_empty());
}

fn affected_flow(name: &str, impact: FlowImpact) -> AffectedFlow {
    AffectedFlow {
        path: PathBuf::from(format!("/flows/{name}.toml")),
        id: name.to_owned().into(),
        name: name.to_owned().into(),
        impact,
    }
}

#[test]
fn delete_warning_copy_matches_item_kind_flow_count_and_impact() {
    let one_missing = [affected_flow("Sync", FlowImpact::MissingRequest)];
    let (title, description, heading) =
        delete_warning_copy(ItemKind::Request("GET"), "Get user", false, &one_missing);
    assert_eq!(title, "Delete request?");
    assert!(description.contains("“Get user”"));
    assert!(description.contains("1 flow"));
    assert!(description.contains("fail if a run reaches it"));
    assert_eq!(heading, "Affected flow (1)");

    let three_missing = [
        affected_flow("A", FlowImpact::MissingRequest),
        affected_flow("B", FlowImpact::MissingRequest),
        affected_flow("C", FlowImpact::MissingRequest),
    ];
    let (title, description, heading) = delete_warning_copy(
        ItemKind::Request("POST"),
        "Create user",
        false,
        &three_missing,
    );
    assert_eq!(title, "Delete request?");
    assert!(description.contains("“Create user”"));
    assert!(description.contains("3 flows"));
    assert!(description.contains("fail if a run reaches it"));
    assert_eq!(heading, "Affected flows (3)");

    let one_different = [affected_flow("Sync", FlowImpact::DifferentRequest)];
    let (_, description, _) =
        delete_warning_copy(ItemKind::Request("GET"), "Get user", false, &one_different);
    assert!(
        description.contains("send another saved request with the same ID if a run reaches it")
    );
    assert!(!description.contains("fail"));

    let mixed_folder = [
        affected_flow("A", FlowImpact::MissingRequest),
        affected_flow("B", FlowImpact::DifferentRequest),
    ];
    let (title, description, heading) =
        delete_warning_copy(ItemKind::Folder, "Users", false, &mixed_folder);
    assert_eq!(title, "Delete folder?");
    assert!(description.contains("“Users”"));
    assert!(description.contains("2 flows"));
    assert!(
        description
            .contains("fail or send another saved request with the same ID if a run reaches it")
    );
    assert_eq!(heading, "Affected flows (2)");

    let (title, description, heading) =
        delete_warning_copy(ItemKind::Collection, "Billing", false, &one_missing);
    assert_eq!(title, "Delete collection?");
    assert!(description.contains("“Billing”"));
    assert!(description.contains("1 flow"));
    assert!(description.contains("fail if a run reaches it"));
    assert_eq!(heading, "Affected flow (1)");

    let two_missing = [
        affected_flow("A", FlowImpact::MissingRequest),
        affected_flow("B", FlowImpact::MissingRequest),
    ];
    let (title, description, heading) =
        delete_warning_copy(ItemKind::Collection, "Billing", true, &two_missing);
    assert_eq!(title, "Remove collection?");
    assert!(description.contains("folder stays on disk"));
    assert!(description.contains("2 flows"));
    assert!(description.contains("fail if a run reaches it"));
    assert_eq!(heading, "Affected flows (2)");

    let (_, description, _) = delete_warning_copy(ItemKind::Request("GET"), "Get user", false, &[]);
    assert!(description.contains("No flows currently use"));
}

#[test]
fn recheck_confirmation_requires_reconfirmation_when_target_or_flows_change() {
    let target = TargetSnapshot {
        path: PathBuf::from("/collections/API/get.toml"),
        label: "Get user".into(),
        kind: ItemKind::Request("GET"),
        linked: false,
        requests: vec![(
            PathBuf::from("/collections/API/get.toml"),
            "req-1".to_owned(),
            true,
        )],
    };
    let affected = vec![affected_flow("Sync", FlowImpact::MissingRequest)];

    assert_eq!(
        recheck_confirmation(&target, &affected, Some(target.clone()), affected.clone()),
        ConfirmationCheck::Proceed
    );

    assert_eq!(
        recheck_confirmation(&target, &affected, None, Vec::new()),
        ConfirmationCheck::Missing
    );

    let replaced_target = TargetSnapshot {
        requests: vec![(
            PathBuf::from("/collections/API/get.toml"),
            "req-2".to_owned(),
            true,
        )],
        ..target.clone()
    };
    assert!(matches!(
        recheck_confirmation(&target, &affected, Some(replaced_target), affected.clone()),
        ConfirmationCheck::Changed { .. }
    ));

    let updated_flows = vec![
        affected_flow("Sync", FlowImpact::MissingRequest),
        affected_flow("Audit", FlowImpact::MissingRequest),
    ];
    assert!(matches!(
        recheck_confirmation(&target, &affected, Some(target.clone()), updated_flows),
        ConfirmationCheck::Changed { .. }
    ));

    assert!(matches!(
        recheck_confirmation(&target, &affected, Some(target.clone()), Vec::new()),
        ConfirmationCheck::Changed { affected, .. } if affected.is_empty()
    ));
}
