use crate::{Block, BlockKind, BlockType, Connection, Field, Flow, TemplateFormat};

fn block(id: &str, kind: BlockKind) -> Block {
    Block {
        id: id.to_owned(),
        title: None,
        x: 0.,
        y: 0.,
        kind,
    }
}

fn connection(from: &str, output: &str, to: &str, input: &str) -> Connection {
    Connection {
        from: from.to_owned(),
        output: output.to_owned(),
        to: to.to_owned(),
        input: input.to_owned(),
    }
}

#[test]
fn saves_blocks_and_connections_as_readable_toml() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::Start {
                    input: String::new(),
                },
            ),
            Block {
                title: Some("Greeting".to_owned()),
                x: 120.5,
                y: -40.,
                ..block(
                    "b2",
                    BlockKind::Template {
                        variables: vec!["name".to_owned()],
                        template: "Hi {{name}}".to_owned(),
                        format: TemplateFormat::Text,
                    },
                )
            },
            block(
                "b3",
                BlockKind::Record {
                    fields: vec![Field {
                        key: "id".to_owned(),
                        value: "7".to_owned(),
                    }],
                },
            ),
        ],
        connections: vec![connection("b1", "data", "b2", "name")],
    };

    let text = toml::to_string_pretty(&flow).unwrap();
    assert!(text.contains("[[blocks]]"), "{text}");
    assert!(text.contains("type = \"template\""), "{text}");
    assert!(text.contains("from = \"b1\""), "{text}");
    // Defaults are left out of the file.
    assert!(!text.contains("format"), "{text}");

    let read: Flow = toml::from_str(&text).unwrap();
    assert_eq!(read, flow);
}

#[test]
fn reads_omitted_settings_as_their_defaults() {
    let flow: Flow = toml::from_str(
        r#"
        [[blocks]]
        id = "b1"
        type = "evaluate"
        x = 10
        y = 20

        [[blocks]]
        id = "b2"
        type = "delay"
        x = 0
        y = 0
        "#,
    )
    .unwrap();

    assert_eq!(
        flow.blocks[0].kind,
        BlockKind::Evaluate {
            variables: vec!["value1".to_owned()],
            expression: String::new(),
        }
    );
    assert_eq!(flow.blocks[0].x, 10.);
    assert_eq!(flow.blocks[1].kind, BlockKind::Delay { milliseconds: 1000 });
    assert!(flow.connections.is_empty());
}

#[test]
fn every_block_type_has_ports_and_survives_a_round_trip() {
    let flow = Flow {
        blocks: BlockType::ALL
            .iter()
            .enumerate()
            .map(|(index, block_type)| block(&format!("b{index}"), block_type.block_kind()))
            .collect(),
        connections: Vec::new(),
    };

    for block in &flow.blocks {
        assert_eq!(block.title(), block.kind.block_type().name());
    }
    flow.check().unwrap();

    let read: Flow = toml::from_str(&toml::to_string_pretty(&flow).unwrap()).unwrap();
    assert_eq!(read, flow);

    let json = serde_json::to_value(&flow).unwrap();
    assert_eq!(serde_json::from_value::<Flow>(json).unwrap(), flow);
}

#[test]
fn names_ports_after_settings() {
    let condition = BlockKind::Condition {
        variables: vec!["a".to_owned(), "b".to_owned()],
        conditions: vec!["a".to_owned(), "b".to_owned()],
    };
    assert_eq!(condition.inputs(), ["a", "b"]);
    assert_eq!(condition.outputs(), ["condition1", "condition2", "default"]);

    let if_block = BlockType::If.block_kind();
    assert_eq!(if_block.inputs(), ["value1", "data"]);
    assert_eq!(if_block.outputs(), ["then", "else"]);

    let list = BlockKind::List {
        items: vec![String::new(), String::new()],
    };
    assert_eq!(list.inputs(), ["item1", "item2"]);
    assert_eq!(BlockType::HttpRequest.block_kind().inputs(), ["send"]);
    assert_eq!(
        BlockType::HttpRequest.block_kind().outputs(),
        ["success", "fail"]
    );
}

#[test]
fn checks_ids_names_and_connections() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockType::String.block_kind()),
            block("b2", BlockType::Evaluate.block_kind()),
            block("b3", BlockType::HttpRequest.block_kind()),
        ],
        connections: vec![connection("b1", "value", "b2", "value1")],
    };
    flow.check().unwrap();

    // HTTP Request inputs follow the request's variables.
    flow.connections
        .push(connection("b1", "value", "b3", "token"));
    flow.check().unwrap();

    let mut duplicate = flow.clone();
    duplicate.blocks[1].id = "b1".to_owned();
    assert!(duplicate.check().unwrap_err().contains("Two blocks"));

    let mut unknown = flow.clone();
    unknown
        .connections
        .push(connection("b9", "value", "b2", "value1"));
    assert!(unknown.check().unwrap_err().contains("unknown block"));

    let mut missing_port = flow.clone();
    missing_port.connections[0].output = "result".to_owned();
    assert!(missing_port.check().unwrap_err().contains("no output"));

    let mut twice = flow.clone();
    twice
        .connections
        .push(connection("b3", "success", "b2", "value1"));
    assert!(twice.check().unwrap_err().contains("more than one"));

    let mut bad_variable = flow.clone();
    bad_variable.blocks[1].kind = BlockKind::Evaluate {
        variables: vec!["two words".to_owned()],
        expression: String::new(),
    };
    assert!(
        bad_variable
            .check()
            .unwrap_err()
            .contains("must start with")
    );

    let mut bad_id = flow;
    bad_id.blocks[0].id = "b.1".to_owned();
    assert!(bad_id.check().unwrap_err().contains("must be letters"));
}

#[test]
fn notes_keep_a_positive_size_and_leave_it_out_until_resized() {
    let mut flow = Flow {
        blocks: vec![block("b1", BlockType::Note.block_kind())],
        connections: Vec::new(),
    };
    flow.check().unwrap();
    assert!(!toml::to_string(&flow).unwrap().contains("width"));

    flow.blocks[0].kind = BlockKind::Note {
        text: "1 · Sign in".to_owned(),
        width: Some(640.),
        height: Some(320.),
    };
    flow.check().unwrap();
    let saved = toml::to_string(&flow).unwrap();
    assert!(saved.contains("width = 640.0"));
    assert_eq!(toml::from_str::<Flow>(&saved).unwrap(), flow);

    flow.blocks[0].kind = BlockKind::Note {
        text: String::new(),
        width: Some(0.),
        height: None,
    };
    assert!(flow.check().unwrap_err().contains("positive size"));
}

#[test]
fn edits_keep_connections_consistent() {
    let mut flow = Flow {
        blocks: vec![
            block("b1", BlockType::String.block_kind()),
            block("b2", BlockType::Evaluate.block_kind()),
            block("b7", BlockType::Display.block_kind()),
        ],
        connections: vec![
            connection("b1", "value", "b2", "value1"),
            connection("b2", "result", "b7", "data"),
        ],
    };
    assert_eq!(flow.next_block_id(), "b8");

    flow.connect(connection("b1", "value", "b7", "data"));
    assert_eq!(flow.connections.len(), 2);
    assert_eq!(flow.connection_into("b7", "data").unwrap().from, "b1");

    flow.rename_port("b2", false, "value1", "name");
    assert_eq!(flow.connections[0].input, "name");

    flow.remove_blocks(&["b1".to_owned()]);
    assert_eq!(flow.blocks.len(), 2);
    assert!(flow.connections.is_empty());
}
