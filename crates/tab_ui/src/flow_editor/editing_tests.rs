use flow::{Block, BlockType, Connection, Flow};
use gpui_kit::{point, size};

use super::editing::{arrange, copy, from_clipboard, paste, to_clipboard};
use super::history::History;

fn block(id: &str, block_type: BlockType, x: f32) -> Block {
    Block {
        id: id.to_owned(),
        title: None,
        x,
        y: 0.,
        kind: block_type.block_kind(),
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

fn sample() -> Flow {
    Flow {
        blocks: vec![
            block("b1", BlockType::String, 0.),
            block("b2", BlockType::Evaluate, 300.),
            block("b3", BlockType::Display, 600.),
        ],
        connections: vec![
            wire("b1", "value", "b2", "value1"),
            wire("b2", "result", "b3", "data"),
        ],
    }
}

#[test]
fn copies_blocks_with_only_their_own_connections() {
    let flow = sample();
    let copied = copy(&flow, &["b2".to_owned(), "b3".to_owned()]);

    assert_eq!(copied.blocks.len(), 2);
    assert_eq!(copied.connections, [wire("b2", "result", "b3", "data")]);
}

#[test]
fn pasting_gives_new_ids_and_moves_the_blocks() {
    let mut flow = sample();
    let copied = copy(&flow, &["b2".to_owned(), "b3".to_owned()]);
    let text = to_clipboard(&copied);

    let pasted = paste(&mut flow, from_clipboard(&text).unwrap(), point(24., 24.));

    assert_eq!(pasted, ["b4", "b5"]);
    assert_eq!(flow.blocks.len(), 5);
    assert_eq!(flow.block("b4").unwrap().x, 324.);
    assert_eq!(flow.block("b5").unwrap().y, 24.);
    assert!(
        flow.connections
            .contains(&wire("b4", "result", "b5", "data"))
    );
    flow.check().unwrap();
}

#[test]
fn ignores_clipboard_text_that_holds_no_blocks() {
    assert!(from_clipboard("hello").is_none());
    assert!(from_clipboard("{\"blocks\": []}").is_none());
    assert!(from_clipboard(&to_clipboard(&Flow::default())).is_none());
}

#[test]
fn arranges_blocks_left_to_right_even_with_a_cycle() {
    let mut flow = sample();
    flow.blocks.reverse();
    flow.blocks.push(block("b4", BlockType::Or, -500.));
    flow.blocks.push(block("b5", BlockType::Start, 900.));
    flow.connections.push(wire("b5", "data", "b4", "first"));
    flow.connections.push(wire("b4", "data", "b1", "value"));
    // A connection back to an earlier block closes a cycle.
    flow.connections.push(wire("b3", "data", "b4", "second"));

    arrange(&mut flow, |_| size(200., 100.));

    let x = |id: &str| flow.block(id).unwrap().x;
    assert!(x("b5") < x("b4"));
    assert!(x("b4") < x("b1"));
    assert!(x("b1") < x("b2"));
    assert!(x("b2") < x("b3"));
}

#[test]
fn undo_and_redo_restore_snapshots_and_join_typing() {
    let mut history = History::default();
    let mut flow = sample();

    history.record(&flow, None);
    flow.blocks.pop();
    // Typing into one setting is one edit.
    history.record(&flow, Some("b1:value".to_owned()));
    flow.blocks[0].title = Some("A".to_owned());
    history.record(&flow, Some("b1:value".to_owned()));
    flow.blocks[0].title = Some("AB".to_owned());

    assert!(history.undo(&mut flow));
    assert_eq!(flow.blocks[0].title, None);
    assert!(history.undo(&mut flow));
    assert_eq!(flow, sample());
    assert!(!history.undo(&mut flow));

    assert!(history.redo(&mut flow));
    assert_eq!(flow.blocks.len(), 2);
    assert!(history.can_redo());
    history.record(&flow, None);
    assert!(!history.can_redo());
}
