use std::{fs, path::PathBuf};

use collection::CollectionRegistry;
use collections_panel_ui::FlowImpact;
use flow::{Block, BlockKind, BlockType, Flow, SavedFlow};

use crate::affected_flows::{FlowSnapshot, find};

fn write_http_request(root: &std::path::Path, relative: &str, id: &str, name: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nschema_version = 1\n\n\
             [request]\ntype = \"http\"\nmethod = \"GET\"\npath = \"https://example.com/{id}\"\n"
        ),
    )
    .unwrap();
    path
}

fn write_grpc_request(root: &std::path::Path, relative: &str, id: &str, name: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nschema_version = 1\n\n\
             [request]\ntype = \"grpc\"\nurl = \"https://example.com\"\nmethod = \"pkg.Svc/Call\"\n"
        ),
    )
    .unwrap();
    path
}

fn flow_with_requests(request_ids: &[&str]) -> Flow {
    let mut blocks = vec![Block {
        id: "b1".to_owned(),
        title: None,
        x: 0.,
        y: 0.,
        kind: BlockType::Start.block_kind(),
    }];
    for (index, request_id) in request_ids.iter().enumerate() {
        blocks.push(Block {
            id: format!("b{}", index + 2),
            title: None,
            x: 0.,
            y: 0.,
            kind: BlockKind::HttpRequest {
                request: (*request_id).to_owned(),
            },
        });
    }

    Flow {
        blocks,
        connections: Vec::new(),
    }
}

fn saved_flow(name: &str, id: &str, request_ids: &[&str]) -> SavedFlow {
    SavedFlow {
        path: PathBuf::from(format!("/flows/{id}.toml")),
        outside_changes: 0,
        id: id.to_owned(),
        name: name.to_owned(),
        schema_version: 1,
        flow: flow_with_requests(request_ids),
    }
}

#[test]
fn finds_and_deduplicates_flows_for_saved_request() {
    let dir = tempfile::tempdir().unwrap();
    let get_user = write_http_request(dir.path(), "API/Users/get.toml", "req-user", "Get User");
    let unused = write_http_request(dir.path(), "API/Users/list.toml", "req-list", "List Users");
    let grpc = write_grpc_request(dir.path(), "API/Users/rpc.toml", "req-grpc", "RPC Call");

    let registry = CollectionRegistry::from_path(dir.path());
    let saved = vec![
        saved_flow("Sync Users", "flow-b", &["req-user", "req-user"]),
        saved_flow("audit users", "flow-a", &["req-user"]),
        saved_flow("Unrelated", "flow-c", &["other", ""]),
        saved_flow("Non-HTTP", "flow-d", &["req-grpc"]),
    ];

    let affected = find(&registry, &get_user, &saved, &[]);
    let names: Vec<_> = affected.iter().map(|flow| flow.name.as_ref()).collect();
    assert_eq!(names, vec!["audit users", "Sync Users"]);
    assert!(
        affected
            .iter()
            .all(|flow| flow.impact == FlowImpact::MissingRequest)
    );

    assert!(find(&registry, &unused, &saved, &[]).is_empty());
    assert!(find(&registry, &grpc, &saved, &[]).is_empty());
}

#[test]
fn finds_affected_flows_when_deleting_folder_or_collection() {
    let dir = tempfile::tempdir().unwrap();
    write_http_request(
        dir.path(),
        "Billing/Invoices/Pay/charge.toml",
        "req-charge",
        "Charge",
    );
    write_http_request(dir.path(), "Billing/Invoices/list.toml", "req-list", "List");
    fs::create_dir_all(dir.path().join("Billing/Empty")).unwrap();

    let registry = CollectionRegistry::from_path(dir.path());
    let saved = vec![
        saved_flow("Checkout", "flow-1", &["req-charge"]),
        saved_flow("Reporting", "flow-2", &["req-list"]),
        saved_flow("Health Check", "flow-3", &["req-other"]),
    ];

    let pay_folder = dir.path().join("Billing/Invoices/Pay");
    let pay_names: Vec<_> = find(&registry, &pay_folder, &saved, &[])
        .into_iter()
        .map(|flow| flow.name)
        .collect();
    assert_eq!(pay_names, vec!["Checkout"]);

    let invoices_folder = dir.path().join("Billing/Invoices");
    let invoice_names: Vec<_> = find(&registry, &invoices_folder, &saved, &[])
        .into_iter()
        .map(|flow| flow.name)
        .collect();
    assert_eq!(invoice_names, vec!["Checkout", "Reporting"]);

    let collection = dir.path().join("Billing");
    let collection_names: Vec<_> = find(&registry, &collection, &saved, &[])
        .into_iter()
        .map(|flow| flow.name)
        .collect();
    assert_eq!(collection_names, vec!["Checkout", "Reporting"]);

    let empty = dir.path().join("Billing/Empty");
    assert!(find(&registry, &empty, &saved, &[]).is_empty());
}

#[test]
fn checks_both_saved_flows_and_open_drafts_and_lists_each_flow_once() {
    let dir = tempfile::tempdir().unwrap();
    let request = write_http_request(dir.path(), "API/login.toml", "req-login", "Login");
    let registry = CollectionRegistry::from_path(dir.path());

    let saved = vec![
        saved_flow("Draft Added", "flow-added", &[]),
        saved_flow("Draft Removed", "flow-removed", &["req-login"]),
        saved_flow("Both Reference", "flow-both", &["req-login"]),
    ];

    let added_draft = flow_with_requests(&["req-login"]);
    let removed_draft = flow_with_requests(&[]);
    let both_draft = flow_with_requests(&["req-login"]);
    let detached_draft = flow_with_requests(&["req-login"]);
    let detached_path = PathBuf::from("/flows/detached.toml");

    let open = vec![
        FlowSnapshot {
            path: &saved[0].path,
            id: &saved[0].id,
            name: &saved[0].name,
            flow: &added_draft,
        },
        FlowSnapshot {
            path: &saved[1].path,
            id: &saved[1].id,
            name: &saved[1].name,
            flow: &removed_draft,
        },
        FlowSnapshot {
            path: &saved[2].path,
            id: &saved[2].id,
            name: &saved[2].name,
            flow: &both_draft,
        },
        FlowSnapshot {
            path: &detached_path,
            id: "flow-detached",
            name: "Detached Draft",
            flow: &detached_draft,
        },
    ];

    let affected = find(&registry, &request, &saved, &open);
    let ids: Vec<_> = affected.iter().map(|flow| flow.id.as_ref()).collect();
    assert_eq!(
        ids,
        vec!["flow-both", "flow-detached", "flow-added", "flow-removed"]
    );
}

#[test]
fn duplicate_request_ids_distinguish_redirected_and_missing_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let active = write_http_request(dir.path(), "Alpha/req.toml", "shared-id", "Active Copy");
    let shadowed = write_http_request(dir.path(), "Beta/req.toml", "shared-id", "Shadowed Copy");
    let unique = write_http_request(dir.path(), "Alpha/only.toml", "unique-id", "Unique");
    let active_with_grpc_fallback = write_http_request(
        dir.path(),
        "Alpha/fallback-grpc.toml",
        "grpc-fallback-id",
        "Active HTTP",
    );
    write_grpc_request(
        dir.path(),
        "Beta/fallback-grpc.toml",
        "grpc-fallback-id",
        "Shadowed gRPC",
    );

    let registry = CollectionRegistry::from_path(dir.path());
    let saved = vec![
        saved_flow("Shared Flow", "flow-1", &["shared-id"]),
        saved_flow("Mixed Flow", "flow-2", &["shared-id", "unique-id"]),
        saved_flow("Fallback gRPC Flow", "flow-3", &["grpc-fallback-id"]),
    ];

    let active_affected = find(&registry, &active, &saved, &[]);
    assert_eq!(active_affected.len(), 2);
    assert!(
        active_affected
            .iter()
            .all(|flow| flow.impact == FlowImpact::DifferentRequest)
    );

    assert!(find(&registry, &shadowed, &saved, &[]).is_empty());

    let unique_affected = find(&registry, &unique, &saved, &[]);
    assert_eq!(unique_affected.len(), 1);
    assert_eq!(unique_affected[0].impact, FlowImpact::MissingRequest);

    let grpc_fallback_affected = find(&registry, &active_with_grpc_fallback, &saved, &[]);
    assert_eq!(grpc_fallback_affected.len(), 1);
    assert_eq!(grpc_fallback_affected[0].impact, FlowImpact::MissingRequest);

    let alpha_collection = dir.path().join("Alpha");
    let alpha_affected = find(&registry, &alpha_collection, &saved, &[]);
    let by_name: std::collections::HashMap<_, _> = alpha_affected
        .iter()
        .map(|flow| (flow.name.as_ref(), flow.impact))
        .collect();
    assert_eq!(by_name["Shared Flow"], FlowImpact::DifferentRequest);
    assert_eq!(by_name["Mixed Flow"], FlowImpact::Mixed);
    assert_eq!(by_name["Fallback gRPC Flow"], FlowImpact::MissingRequest);
}
