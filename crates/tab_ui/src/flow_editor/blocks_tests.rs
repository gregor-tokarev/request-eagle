use flow::{Block, BlockKind, BlockType, Connection, Flow, StartInput};

use super::blocks::{http_runs_on_start_at, schema_summary, start_input_caption};

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
fn summarizes_what_a_schema_checks() {
    assert_eq!(
        schema_summary(r#"{"type": "object", "required": ["body", "id"]}"#),
        Ok("object · requires body, id".to_owned())
    );
    assert_eq!(
        schema_summary(r#"{"type": ["string", "null"]}"#),
        Ok("string or null".to_owned())
    );
    assert_eq!(
        schema_summary(r#"{"properties": {"a": {}, "b": {}}}"#),
        Ok("2 properties".to_owned())
    );
    assert_eq!(schema_summary("{}"), Ok("Checks a JSON Schema".to_owned()));
    assert_eq!(schema_summary("true"), Ok("Accepts anything".to_owned()));
    assert!(schema_summary("{\"type\": ").is_err());
    assert!(schema_summary("[1]").is_err());
}

#[test]
fn start_caption_explains_non_json_inputs() {
    assert_eq!(start_input_caption(&StartInput::check("")), None);
    assert_eq!(
        start_input_caption(&StartInput::check(r#"{"name":"Gregor"}"#)),
        None
    );
    assert_eq!(start_input_caption(&StartInput::check("42")), None);
    assert_eq!(
        start_input_caption(&StartInput::check(r#"{"name":"Gregor",}"#)),
        Some("Invalid JSON · sends as text")
    );
    assert_eq!(
        start_input_caption(&StartInput::check("hello")),
        Some("Not JSON · sends as text")
    );
}

#[test]
fn http_runs_on_start_at_only_marks_unconnected_http_blocks() {
    let mut flow = Flow {
        blocks: vec![
            block("start", BlockType::Start.block_kind()),
            block("text", BlockType::String.block_kind()),
            block("display", BlockType::Display.block_kind()),
            block(
                "http_unconnected",
                BlockKind::HttpRequest {
                    request: "req-1".to_owned(),
                },
            ),
            block(
                "http_outgoing",
                BlockKind::HttpRequest {
                    request: "req-2".to_owned(),
                },
            ),
            block(
                "http_send",
                BlockKind::HttpRequest {
                    request: "req-3".to_owned(),
                },
            ),
            block(
                "http_var",
                BlockKind::HttpRequest {
                    request: "req-4".to_owned(),
                },
            ),
        ],
        connections: vec![
            wire("http_outgoing", "success", "display", "data"),
            wire("start", "data", "http_send", "send"),
            wire("text", "value", "http_var", "token"),
        ],
    };

    assert!(!http_runs_on_start_at(&flow, 0));
    assert!(!http_runs_on_start_at(&flow, 1));
    assert!(!http_runs_on_start_at(&flow, 2));
    assert!(http_runs_on_start_at(&flow, 3));
    assert!(http_runs_on_start_at(&flow, 4));
    assert!(!http_runs_on_start_at(&flow, 5));
    assert!(!http_runs_on_start_at(&flow, 6));

    // Connecting Send stops auto-start immediately.
    flow.connect(wire("start", "data", "http_unconnected", "send"));
    assert!(!http_runs_on_start_at(&flow, 3));

    // Disconnecting restores auto-start immediately.
    flow.connections.retain(|c| c.to != "http_unconnected");
    assert!(http_runs_on_start_at(&flow, 3));

    // Deleting an upstream block restores auto-start on its HTTP target.
    flow.remove_blocks(&["text".to_owned()]);
    let http_var_index = flow.blocks.iter().position(|b| b.id == "http_var").unwrap();
    assert!(http_runs_on_start_at(&flow, http_var_index));
}

#[test]
fn http_runs_on_start_at_matches_each_instance_when_block_ids_repeat() {
    // Two HTTP blocks share an ID: an incoming wire to that ID makes both wait
    // in Graph::new, and removing it makes both auto-start.
    let mut duplicate_http = Flow {
        blocks: vec![
            block("start", BlockType::Start.block_kind()),
            block(
                "dup",
                BlockKind::HttpRequest {
                    request: "req-1".to_owned(),
                },
            ),
            block(
                "dup",
                BlockKind::HttpRequest {
                    request: "req-2".to_owned(),
                },
            ),
        ],
        connections: vec![wire("start", "data", "dup", "send")],
    };
    assert!(!http_runs_on_start_at(&duplicate_http, 1));
    assert!(!http_runs_on_start_at(&duplicate_http, 2));

    duplicate_http.connections.clear();
    assert!(http_runs_on_start_at(&duplicate_http, 1));
    assert!(http_runs_on_start_at(&duplicate_http, 2));

    // A non-HTTP block sharing an ID is still never marked as an HTTP block.
    let mixed_duplicate = Flow {
        blocks: vec![
            block("dup", BlockType::Start.block_kind()),
            block(
                "dup",
                BlockKind::HttpRequest {
                    request: "req-1".to_owned(),
                },
            ),
        ],
        connections: Vec::new(),
    };
    assert!(!http_runs_on_start_at(&mixed_duplicate, 0));
    assert!(http_runs_on_start_at(&mixed_duplicate, 1));
}
