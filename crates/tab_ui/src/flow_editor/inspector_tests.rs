use flow::{Block, BlockKind, BlockType, Connection, Flow};

use super::inspector::{List, Setting, apply, change_list};

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
