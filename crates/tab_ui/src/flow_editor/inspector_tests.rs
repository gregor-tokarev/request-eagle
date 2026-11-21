use std::path::PathBuf;

use flow::{Block, BlockKind, BlockType, Connection, Flow};
use request::{Auth, HttpRequest};

use super::{
    editor::FlowRequest,
    inspector::{List, RequestList, Setting, apply, change_list, invalid, request_list},
};

fn block(id: &str, kind: BlockKind) -> Block {
    Block {
        id: id.to_owned(),
        title: None,
        x: 0.,
        y: 0.,
        kind,
    }
}

fn wire(from: &str, output: &str, to: &str, input: &str) -> Connection {
    Connection {
        from: from.to_owned(),
        output: output.to_owned(),
        to: to.to_owned(),
        input: input.to_owned(),
    }
}

#[test]
fn renaming_a_variable_keeps_its_connection() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockType::String.block_kind()),
            block("b2", BlockType::Evaluate.block_kind()),
        ],
        connections: vec![wire("b1", "value", "b2", "value1")],
    };

    apply(&mut flow, "b2", Setting::Variable(0), "response".to_owned());
    apply(
        &mut flow,
        "b2",
        Setting::Expression,
        "response.body".to_owned(),
    );
    apply(&mut flow, "b2", Setting::Title, "Read body".to_owned());

    assert_eq!(flow.connections, [wire("b1", "value", "b2", "response")]);
    assert_eq!(
        flow.blocks[1].kind,
        BlockKind::Evaluate {
            variables: vec!["response".to_owned()],
            expression: "response.body".to_owned(),
        }
    );
    assert_eq!(flow.blocks[1].title(), "Read body");

    apply(&mut flow, "b2", Setting::Title, "  ".to_owned());
    assert_eq!(flow.blocks[1].title(), "Evaluate");
}

#[test]
fn numbers_and_delays_keep_their_last_valid_value() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockKind::Number { value: 1. }),
            block("b2", BlockKind::Delay { milliseconds: 10 }),
        ],
        connections: Vec::new(),
    };

    apply(&mut flow, "b1", Setting::Number, "2.5".to_owned());
    apply(&mut flow, "b1", Setting::Number, "2.5x".to_owned());
    apply(&mut flow, "b2", Setting::Milliseconds, "250".to_owned());
    apply(&mut flow, "b2", Setting::Milliseconds, "-1".to_owned());

    assert_eq!(flow.blocks[0].kind, BlockKind::Number { value: 2.5 });
    assert_eq!(flow.blocks[1].kind, BlockKind::Delay { milliseconds: 250 });

    // The inspector reports what it could not apply.
    assert!(invalid(&flow, "b1", Setting::Number, "2.5x").is_some());
    assert!(invalid(&flow, "b1", Setting::Number, " 3 ").is_none());
    assert!(invalid(&flow, "b2", Setting::Milliseconds, "-1").is_some());
}

#[test]
fn a_rename_onto_another_ports_name_is_refused() {
    let flow = Flow {
        blocks: vec![
            block("b1", BlockType::String.block_kind()),
            block(
                "b2",
                BlockKind::Record {
                    fields: vec![
                        flow::Field {
                            key: "a".to_owned(),
                            value: String::new(),
                        },
                        flow::Field {
                            key: "ab".to_owned(),
                            value: String::new(),
                        },
                    ],
                },
            ),
            block(
                "b3",
                BlockKind::If {
                    variables: vec!["value1".to_owned()],
                    condition: "true".to_owned(),
                },
            ),
        ],
        connections: vec![
            wire("b1", "value", "b2", "a"),
            wire("b1", "value", "b2", "ab"),
        ],
    };

    // Typing "abc" over "ab" passes through "a", which is taken.
    assert!(invalid(&flow, "b2", Setting::FieldKey(1), "a").is_some());
    assert!(invalid(&flow, "b2", Setting::FieldKey(1), "abc").is_none());
    assert!(invalid(&flow, "b2", Setting::FieldKey(1), " ").is_some());
    assert!(invalid(&flow, "b3", Setting::Variable(0), "data").is_some());
    assert!(invalid(&flow, "b3", Setting::Variable(0), "value1").is_none());
}

#[test]
fn removing_a_numbered_entry_moves_later_connections_up() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockType::String.block_kind()),
            block(
                "b2",
                BlockKind::List {
                    items: vec![String::new(), String::new(), String::new()],
                },
            ),
            block(
                "b3",
                BlockKind::Condition {
                    variables: vec!["value1".to_owned()],
                    conditions: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
                },
            ),
            block("b4", BlockType::Display.block_kind()),
        ],
        connections: vec![
            wire("b1", "value", "b2", "item1"),
            wire("b1", "value", "b2", "item3"),
            wire("b3", "condition2", "b4", "data"),
        ],
    };

    change_list(&mut flow, "b2", List::Items, Some(0));
    assert_eq!(flow.connections.len(), 2);
    assert!(
        flow.connections
            .contains(&wire("b1", "value", "b2", "item2"))
    );

    change_list(&mut flow, "b3", List::Conditions, Some(0));
    assert!(
        flow.connections
            .contains(&wire("b3", "condition1", "b4", "data"))
    );
    assert_eq!(
        flow.blocks[2].kind,
        BlockKind::Condition {
            variables: vec!["value1".to_owned()],
            conditions: vec!["b".to_owned(), "c".to_owned()],
        }
    );
    flow.check().unwrap();
}

#[test]
fn adding_entries_picks_unused_names() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockType::Evaluate.block_kind()),
            block("b2", BlockType::Record.block_kind()),
            block("b3", BlockType::Output.block_kind()),
        ],
        connections: vec![wire("b1", "result", "b3", "result")],
    };

    change_list(&mut flow, "b1", List::Variables, None);
    change_list(&mut flow, "b2", List::Fields, None);
    change_list(&mut flow, "b3", List::Outputs, None);
    change_list(&mut flow, "b3", List::Outputs, Some(0));

    assert_eq!(flow.blocks[0].kind.inputs(), ["value1", "value2"]);
    assert_eq!(flow.blocks[1].kind.inputs(), ["key1", "key2"]);
    assert_eq!(flow.blocks[2].kind.inputs(), ["output1"]);
    // Removing an output name drops its connection.
    assert!(flow.connections.is_empty());
}

fn saved_request(id: &str, name: &str, path: &str, location: &str) -> FlowRequest {
    FlowRequest {
        id: id.to_owned(),
        name: name.to_owned().into(),
        location: location.to_owned().into(),
        collection: PathBuf::from("/collections/API"),
        request: HttpRequest {
            path: path.to_owned(),
            ..HttpRequest::default()
        },
        collection_auth: Auth::None,
    }
}

#[test]
fn request_list_distinguishes_no_saved_requests_from_no_search_matches() {
    assert!(matches!(
        request_list(Vec::new(), ""),
        RequestList::NoSavedRequests
    ));
    assert!(matches!(
        request_list(Vec::new(), "users"),
        RequestList::NoSavedRequests
    ));

    let saved = vec![
        saved_request("r1", "List Users", "https://api.example.com/users", "API"),
        saved_request(
            "r2",
            "Health",
            "https://status.example.com/ok",
            "Ops › Checks",
        ),
    ];

    let RequestList::Matches(all) = request_list(saved.clone(), "   ") else {
        panic!("empty search should list saved requests");
    };
    let ids: Vec<&str> = all.iter().map(|request| request.id.as_str()).collect();
    assert_eq!(ids, ["r1", "r2"]);

    let RequestList::Matches(by_name) = request_list(saved.clone(), "USERS") else {
        panic!("name search should match case-insensitively");
    };
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].id, "r1");

    let RequestList::Matches(by_url) = request_list(saved.clone(), "status.example.com") else {
        panic!("URL search should match");
    };
    assert_eq!(by_url.len(), 1);
    assert_eq!(by_url[0].id, "r2");

    let RequestList::Matches(by_location) = request_list(saved.clone(), "checks") else {
        panic!("location search should match");
    };
    assert_eq!(by_location.len(), 1);
    assert_eq!(by_location[0].id, "r2");

    assert!(matches!(
        request_list(saved, "does-not-exist"),
        RequestList::NoMatches
    ));
}

#[test]
fn request_list_caps_matches_at_thirty() {
    let saved: Vec<_> = (0..35)
        .map(|index| saved_request(&format!("r{index}"), &format!("Req {index}"), "/ok", "API"))
        .collect();

    let RequestList::Matches(matches) = request_list(saved, "") else {
        panic!("saved requests should match empty search");
    };
    assert_eq!(matches.len(), 30);
    assert_eq!(matches[0].id, "r0");
    assert_eq!(matches[29].id, "r29");
}
