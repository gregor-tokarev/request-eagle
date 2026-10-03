use std::{collections::HashSet, fs};

use collection::CollectionRegistry;

use super::tree::{CollectionTree, ItemKind};

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
