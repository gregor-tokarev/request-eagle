use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use collection::{FileEntry, SharedSettings};
use gpui_kit::SharedString;
use request::{
    Auth, BearerAuth, GrpcRequest, HttpRequest, Method, Request, RequestScripts, WebSocketRequest,
};

use super::run::RunRequest;
use super::runner::{
    Preparing, SequenceItem, is_original_sequence, merge_sequence, resolve_prepared_inputs,
    resolve_prepared_requests, runnable,
};

fn file_entry(id: &str, name: &str, path: &str, request: Request) -> FileEntry {
    let mut entry: FileEntry = serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "schema_version": 1,
        "request": request,
    }))
    .unwrap();
    entry.path = PathBuf::from(path);
    entry
}

fn http_entry(
    id: &str,
    name: &str,
    path: &str,
    method: Method,
    folders: &[&str],
) -> (FileEntry, Vec<SharedString>) {
    (
        file_entry(
            id,
            name,
            path,
            Request::Http(HttpRequest {
                method,
                path: format!("https://example.com/{name}"),
                ..Default::default()
            }),
        ),
        folders.iter().map(|&folder| folder.into()).collect(),
    )
}

fn grpc_entry(id: &str, path: &str) -> (FileEntry, Vec<SharedString>) {
    (
        file_entry(id, id, path, Request::Grpc(GrpcRequest::default())),
        Vec::new(),
    )
}

fn websocket_entry(id: &str, path: &str) -> (FileEntry, Vec<SharedString>) {
    (
        file_entry(
            id,
            id,
            path,
            Request::WebSocket(WebSocketRequest::default()),
        ),
        Vec::new(),
    )
}

fn selected_sequence(requests: &[RunRequest]) -> Vec<SequenceItem> {
    requests
        .iter()
        .cloned()
        .map(|request| SequenceItem {
            request,
            selected: true,
        })
        .collect()
}

fn ids_and_selected(sequence: &[SequenceItem]) -> Vec<(&str, bool)> {
    sequence
        .iter()
        .map(|item| (item.request.id.as_ref(), item.selected))
        .collect()
}

fn paths_and_selected(sequence: &[SequenceItem]) -> Vec<(&Path, bool)> {
    sequence
        .iter()
        .map(|item| (item.request.path.as_path(), item.selected))
        .collect()
}

#[test]
fn saving_into_an_empty_collection_populates_and_selects_requests() {
    let (original, other) = runnable([]);
    let sequence = selected_sequence(&original);
    assert!(sequence.is_empty());
    assert_eq!(other, 0);

    let (next_original, other) = runnable([http_entry(
        "req-1",
        "Get users",
        "/col/get-users.toml",
        Method::Get,
        &[],
    )]);
    let next_sequence = merge_sequence(&sequence, &original, &next_original, None);

    assert_eq!(other, 0);
    assert_eq!(ids_and_selected(&next_sequence), [("req-1", true)]);
    assert!(is_original_sequence(&next_sequence, &next_original));
}

#[test]
fn uncustomized_sequence_follows_sidebar_inserts_and_reorders() {
    let (original, _) = runnable([
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("b", "B", "/col/b.toml", Method::Get, &[]),
    ]);
    let sequence = selected_sequence(&original);

    // Saving a new request into a folder ahead of B keeps sidebar order.
    let (with_folder_insert, _) = runnable([
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("c", "C", "/col/users/c.toml", Method::Post, &["users"]),
        http_entry("b", "B", "/col/b.toml", Method::Get, &[]),
    ]);
    let after_insert = merge_sequence(&sequence, &original, &with_folder_insert, None);
    assert_eq!(
        ids_and_selected(&after_insert),
        [("a", true), ("c", true), ("b", true)]
    );
    assert!(is_original_sequence(&after_insert, &with_folder_insert));

    // Reordering in the sidebar updates an uncustomized runner too.
    let (reordered, _) = runnable([
        http_entry("b", "B", "/col/b.toml", Method::Get, &[]),
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("c", "C", "/col/users/c.toml", Method::Post, &["users"]),
    ]);
    let after_reorder = merge_sequence(&after_insert, &with_folder_insert, &reordered, None);
    assert_eq!(
        ids_and_selected(&after_reorder),
        [("b", true), ("a", true), ("c", true)]
    );
    assert!(is_original_sequence(&after_reorder, &reordered));
}

#[test]
fn customized_sequence_preserves_order_and_selection_across_edits_adds_and_deletes() {
    let (original, _) = runnable([
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("b", "B", "/col/b.toml", Method::Get, &[]),
        http_entry("c", "C", "/col/c.toml", Method::Get, &[]),
    ]);

    // Reorder to [C, A, B] and uncheck A.
    let sequence = vec![
        SequenceItem {
            request: original[2].clone(),
            selected: true,
        },
        SequenceItem {
            request: original[0].clone(),
            selected: false,
        },
        SequenceItem {
            request: original[1].clone(),
            selected: true,
        },
    ];
    assert!(!is_original_sequence(&sequence, &original));

    // Delete B, rename/move/change method of A, and add new D ahead of C.
    let (next_original, _) = runnable([
        http_entry("d", "D", "/col/d.toml", Method::Put, &[]),
        http_entry("c", "C", "/col/c.toml", Method::Get, &[]),
        http_entry("a", "Auth", "/col/auth/a.toml", Method::Post, &["auth"]),
    ]);
    let merged = merge_sequence(&sequence, &original, &next_original, None);

    assert_eq!(
        ids_and_selected(&merged),
        [("c", true), ("a", false), ("d", true)]
    );
    assert_eq!(merged[1].request.name.as_ref(), "Auth");
    assert_eq!(merged[1].request.request.method, Method::Post);
    assert_eq!(merged[1].request.path, PathBuf::from("/col/auth/a.toml"));
    assert_eq!(merged[1].request.folders, vec![SharedString::from("auth")]);
    assert!(!is_original_sequence(&merged, &next_original));
}

#[test]
fn duplicate_ids_match_by_path_and_preserve_selection_across_refresh_and_moves() {
    let (original, _) = runnable([
        http_entry("dup", "First", "/col/a.toml", Method::Get, &[]),
        http_entry("dup", "Second", "/col/b.toml", Method::Post, &[]),
    ]);

    // Reordering two files with the same ID counts as customized.
    let reordered = vec![
        SequenceItem {
            request: original[1].clone(),
            selected: true,
        },
        SequenceItem {
            request: original[0].clone(),
            selected: false,
        },
    ];
    assert!(!is_original_sequence(&reordered, &original));

    // Refreshing after adding a third file keeps each duplicate-ID file distinct.
    let (with_third, _) = runnable([
        http_entry("dup", "First", "/col/a.toml", Method::Get, &[]),
        http_entry("dup", "Second", "/col/b.toml", Method::Post, &[]),
        http_entry("c", "Third", "/col/c.toml", Method::Put, &[]),
    ]);
    let merged = merge_sequence(&reordered, &original, &with_third, None);
    assert_eq!(
        paths_and_selected(&merged),
        [
            (Path::new("/col/b.toml"), true),
            (Path::new("/col/a.toml"), false),
            (Path::new("/col/c.toml"), true),
        ]
    );
    assert_eq!(merged[0].request.name.as_ref(), "Second");
    assert_eq!(merged[1].request.name.as_ref(), "First");

    // Moving /col/a.toml -> /col/sub/a.toml preserves its unchecked state.
    let (after_move, _) = runnable([
        http_entry("dup", "First", "/col/sub/a.toml", Method::Get, &["sub"]),
        http_entry("dup", "Second", "/col/b.toml", Method::Post, &[]),
        http_entry("c", "Third", "/col/c.toml", Method::Put, &[]),
    ]);
    let moved = merge_sequence(
        &merged,
        &with_third,
        &after_move,
        Some((Path::new("/col/a.toml"), Path::new("/col/sub/a.toml"))),
    );
    assert_eq!(
        paths_and_selected(&moved),
        [
            (Path::new("/col/b.toml"), true),
            (Path::new("/col/sub/a.toml"), false),
            (Path::new("/col/c.toml"), true),
        ]
    );

    // Deleting /col/b.toml removes only that file, keeping /col/sub/a.toml unchecked.
    let (after_delete, _) = runnable([
        http_entry("dup", "First", "/col/sub/a.toml", Method::Get, &["sub"]),
        http_entry("c", "Third", "/col/c.toml", Method::Put, &[]),
    ]);
    let deleted = merge_sequence(&moved, &after_move, &after_delete, None);
    assert_eq!(
        paths_and_selected(&deleted),
        [
            (Path::new("/col/sub/a.toml"), false),
            (Path::new("/col/c.toml"), true),
        ]
    );
}

#[test]
fn unchecking_all_requests_keeps_them_unchecked_when_new_request_arrives() {
    let (original, _) = runnable([http_entry("a", "A", "/col/a.toml", Method::Get, &[])]);
    let sequence = vec![SequenceItem {
        request: original[0].clone(),
        selected: false,
    }];

    let (next_original, _) = runnable([
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("b", "B", "/col/b.toml", Method::Post, &[]),
    ]);
    let merged = merge_sequence(&sequence, &original, &next_original, None);

    assert_eq!(ids_and_selected(&merged), [("a", false), ("b", true)]);
}

#[test]
fn deleting_all_requests_empties_sequence_and_future_saves_are_selected() {
    let (original, _) = runnable([http_entry("a", "A", "/col/a.toml", Method::Get, &[])]);
    let sequence = vec![SequenceItem {
        request: original[0].clone(),
        selected: false,
    }];

    let (empty_original, _) = runnable([]);
    let empty_sequence = merge_sequence(&sequence, &original, &empty_original, None);
    assert!(empty_sequence.is_empty());
    assert!(is_original_sequence(&empty_sequence, &empty_original));

    let (next_original, _) = runnable([http_entry("b", "B", "/col/b.toml", Method::Get, &[])]);
    let populated = merge_sequence(&empty_sequence, &empty_original, &next_original, None);
    assert_eq!(ids_and_selected(&populated), [("b", true)]);
}

#[test]
fn non_http_requests_are_counted_without_entering_the_run_sequence() {
    let (original, other) = runnable([
        grpc_entry("g1", "/col/g1.toml"),
        http_entry("h1", "HTTP", "/col/h1.toml", Method::Get, &[]),
        websocket_entry("w1", "/col/w1.toml"),
    ]);

    assert_eq!(other, 2);
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].id.as_ref(), "h1");
}

#[test]
fn preparing_tracks_collection_and_request_relocations() {
    let (original, _) = runnable([
        http_entry("a", "A", "/col/a.toml", Method::Get, &[]),
        http_entry("b", "B", "/col/sub/b.toml", Method::Post, &["sub"]),
    ]);
    let mut preparing = Preparing::new(
        original,
        PathBuf::from("/col"),
        HashMap::new(),
        SharedSettings::default(),
        None,
        None,
    );

    // Renaming the collection updates the collection dir and all request paths.
    preparing.relocate(
        Path::new("/col"),
        Path::new("/col-renamed"),
        Path::new("/col-renamed"),
        None,
    );
    assert!(preparing.relocated);
    assert_eq!(preparing.collection, PathBuf::from("/col-renamed"));
    assert_eq!(
        preparing.selected[0].path,
        PathBuf::from("/col-renamed/a.toml")
    );
    assert_eq!(
        preparing.selected[1].path,
        PathBuf::from("/col-renamed/sub/b.toml")
    );

    // Moving a request (even out of the collection) rebases its path for disk retry.
    preparing.relocated = false;
    preparing.relocate_paths(Path::new("/col-renamed/a.toml"), Path::new("/other/a.toml"));
    assert!(preparing.relocated);
    assert_eq!(preparing.selected[0].path, PathBuf::from("/other/a.toml"));
    assert!(preparing.removed.is_empty());
}

#[test]
fn deleting_parent_folder_only_falls_back_for_files_that_existed_before_delete() {
    let (original, _) = runnable([
        http_entry("a", "AlreadyGone", "/col/sub/a.toml", Method::Get, &["sub"]),
        http_entry("b", "Existed", "/col/sub/b.toml", Method::Post, &["sub"]),
        http_entry("c", "ExternalMissing", "/col/c.toml", Method::Delete, &[]),
    ]);
    let mut preparing = Preparing::new(
        original.clone(),
        PathBuf::from("/col"),
        HashMap::new(),
        SharedSettings::default(),
        None,
        None,
    );

    // A was deleted externally before Run; B existed when the user deleted /col/sub
    // in the app while preparing. Only B is in removed_files.
    let removed_files = HashSet::from([PathBuf::from("/col/sub/b.toml")]);
    preparing.note_deleted(Path::new("/col/sub"), &removed_files, false);
    assert!(!preparing.collection_removed);
    assert!(!preparing.removed.contains(&0));
    assert!(preparing.removed.contains(&1));

    // An external reload reporting C missing does not mark C for fallback.
    let previous_sequence = selected_sequence(&original);
    let (remaining, _) = runnable([]);
    preparing.follow_refresh(&previous_sequence, &remaining);
    assert!(!preparing.removed.contains(&2));

    let (requests, missing) = resolve_prepared_requests(
        &preparing.selected,
        vec![None, None, None],
        &preparing.removed,
        Path::new("/col"),
    );

    let run_ids: Vec<&str> = requests.iter().map(|r| r.id.as_ref()).collect();
    assert_eq!(run_ids, ["b"]);
    assert_eq!(
        missing,
        vec![
            SharedString::from("AlreadyGone"),
            SharedString::from("ExternalMissing")
        ]
    );
}

#[test]
fn deleting_or_moving_a_selected_request_does_not_redirect_to_unchecked_duplicate_id() {
    let (original, _) = runnable([
        http_entry("dup", "SelectedA", "/col/a.toml", Method::Get, &[]),
        http_entry("dup", "UncheckedB", "/col/b.toml", Method::Delete, &[]),
    ]);
    let previous_sequence = vec![
        SequenceItem {
            request: original[0].clone(),
            selected: true,
        },
        SequenceItem {
            request: original[1].clone(),
            selected: false,
        },
    ];

    // Case 1: Only A is selected; user deletes A while preparing.
    let mut preparing_delete = Preparing::new(
        vec![original[0].clone()],
        PathBuf::from("/col"),
        HashMap::new(),
        SharedSettings::default(),
        None,
        None,
    );
    let removed_files = HashSet::from([PathBuf::from("/col/a.toml")]);
    preparing_delete.note_deleted(Path::new("/col/a.toml"), &removed_files, false);
    preparing_delete.follow_refresh(&previous_sequence, &[original[1].clone()]);
    assert!(!preparing_delete.relocated);
    assert_eq!(
        preparing_delete.selected[0].path,
        PathBuf::from("/col/a.toml")
    );
    assert_eq!(preparing_delete.selected[0].name.as_ref(), "SelectedA");

    // Case 2: Only A is selected; user moves A out of the collection while preparing.
    let mut preparing_move = Preparing::new(
        vec![original[0].clone()],
        PathBuf::from("/col"),
        HashMap::new(),
        SharedSettings::default(),
        None,
        None,
    );
    preparing_move.relocate_paths(Path::new("/col/a.toml"), Path::new("/other/a.toml"));
    preparing_move.follow_refresh(&previous_sequence, &[original[1].clone()]);
    assert_eq!(
        preparing_move.selected[0].path,
        PathBuf::from("/other/a.toml")
    );
    assert_eq!(preparing_move.selected[0].name.as_ref(), "SelectedA");

    // Case 3: Only A is selected; A was deleted externally before Run and reload arrives.
    let mut preparing_external = Preparing::new(
        vec![original[0].clone()],
        PathBuf::from("/col"),
        HashMap::new(),
        SharedSettings::default(),
        None,
        None,
    );
    preparing_external.follow_refresh(&previous_sequence, &[original[1].clone()]);
    assert!(!preparing_external.relocated);
    assert_eq!(
        preparing_external.selected[0].path,
        PathBuf::from("/col/a.toml")
    );
    assert!(preparing_external.removed.is_empty());
}

#[test]
fn deleting_a_collection_while_preparing_preserves_its_variables_auth_and_scripts() {
    let (original, _) = runnable([http_entry("a", "A", "/col/a.toml", Method::Get, &[])]);
    let captured_values = HashMap::from([("baseUrl".into(), "https://api.example.com".into())]);
    let captured_settings = SharedSettings {
        scripts: RequestScripts {
            pre_request: "pm.variables.set('k', 'v')".into(),
            post_response: String::new(),
        },
        auth: Auth::Bearer(BearerAuth {
            token: "secret".into(),
        }),
    };
    let mut preparing = Preparing::new(
        original,
        PathBuf::from("/col"),
        captured_values.clone(),
        captured_settings.clone(),
        None,
        None,
    );

    let removed_files = HashSet::from([PathBuf::from("/col/a.toml")]);
    preparing.note_deleted(Path::new("/col"), &removed_files, false);
    assert!(preparing.collection_removed);
    assert!(preparing.removed.contains(&0));

    // Background disk reads saw empty collection files after delete, while the
    // global environment still loaded normally.
    let global_env = HashMap::from([("env".into(), "prod".into())]);
    let (collection_values, environment_values, error, settings) = resolve_prepared_inputs(
        preparing.collection_removed,
        preparing.collection_values,
        preparing.collection_settings,
        Ok(HashMap::new()),
        Ok(global_env.clone()),
        Ok(SharedSettings::default()),
    );

    assert_eq!(collection_values, captured_values);
    assert_eq!(environment_values, global_env);
    assert_eq!(error, None);
    assert_eq!(settings, Ok(captured_settings));
}
