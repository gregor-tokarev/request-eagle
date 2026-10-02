use std::{collections::HashMap, time::Duration};

use request::{HttpRequest, Method, RequestExecutor, RequestPreferences};
use serde_json::{Value, json};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{
    Block, BlockKind, BlockType, Connection, Field, Flow, RunEvent, RunOptions, RunSummary,
    SavedRequest, TemplateFormat, run,
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

fn evaluate(variables: &[&str], expression: &str) -> BlockKind {
    BlockKind::Evaluate {
        variables: variables.iter().map(|name| name.to_string()).collect(),
        expression: expression.to_owned(),
    }
}

fn output(names: &[&str]) -> BlockKind {
    BlockKind::Output {
        names: names.iter().map(|name| name.to_string()).collect(),
    }
}

fn executor() -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        timeout_ms: 5_000,
        ..RequestPreferences::default()
    })
    .unwrap()
}

fn options() -> RunOptions {
    RunOptions {
        input: None,
        requests: HashMap::new(),
        executor: executor(),
    }
}

/// Run a flow, keeping every event.
fn run_flow(flow: Flow, options: RunOptions) -> (RunSummary, Vec<RunEvent>) {
    let mut events = Vec::new();
    let summary = smol::block_on(run(flow, options, |event| events.push(event)));

    (summary, events)
}

fn finished<'a>(events: &'a [RunEvent], block: &str) -> Vec<&'a crate::BlockRun> {
    events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Finished(run) if run.block == block => Some(run),
            _ => None,
        })
        .collect()
}

#[test]
fn sources_run_and_send_their_values_downstream() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::String {
                    value: "world".to_owned(),
                },
            ),
            block("b2", BlockKind::Number { value: 3. }),
            block(
                "b3",
                evaluate(&["name", "count"], "\"hello \" & name & \" \" & count"),
            ),
            block("b4", output(&["greeting", "count"])),
        ],
        connections: vec![
            wire("b1", "value", "b3", "name"),
            wire("b2", "value", "b3", "count"),
            wire("b3", "result", "b4", "greeting"),
            wire("b2", "value", "b4", "count"),
        ],
    };

    let (summary, events) = run_flow(flow, options());

    assert_eq!(summary.outputs["greeting"], "hello world 3");
    assert_eq!(summary.outputs["count"], 3);
    assert_eq!(summary.failures, 0);
    assert!(summary.stopped.is_none());
    // The evaluation waited for both of its inputs.
    assert_eq!(finished(&events, "b3").len(), 1);
}

#[test]
fn start_sends_the_run_input_or_its_own() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::Start {
                    input: "{\"user\": {\"id\": 5}}".to_owned(),
                },
            ),
            block(
                "b2",
                BlockKind::Select {
                    path: "user.id".to_owned(),
                },
            ),
            block("b3", output(&["id"])),
        ],
        connections: vec![
            wire("b1", "data", "b2", "data"),
            wire("b2", "value", "b3", "id"),
        ],
    };

    let (summary, _) = run_flow(flow.clone(), options());
    assert_eq!(summary.outputs["id"], 5);

    let (summary, _) = run_flow(
        flow,
        RunOptions {
            input: Some(json!({"user": {"id": "from run"}})),
            ..options()
        },
    );
    assert_eq!(summary.outputs["id"], "from run");
}

#[test]
fn if_and_condition_route_their_data() {
    let flow = Flow {
        blocks: vec![
            block("b1", BlockKind::Number { value: 7. }),
            block(
                "b2",
                BlockKind::If {
                    variables: vec!["n".to_owned()],
                    condition: "n > 5".to_owned(),
                },
            ),
            block(
                "b3",
                BlockKind::String {
                    value: "payload".to_owned(),
                },
            ),
            block(
                "b4",
                BlockKind::Condition {
                    variables: vec!["n".to_owned()],
                    conditions: vec!["n < 0".to_owned(), "n % 2 = 1".to_owned()],
                },
            ),
            block("b5", output(&["then", "else", "odd", "default"])),
        ],
        connections: vec![
            wire("b1", "value", "b2", "n"),
            wire("b3", "value", "b2", "data"),
            wire("b2", "then", "b5", "then"),
            wire("b2", "else", "b5", "else"),
            wire("b1", "value", "b4", "n"),
            wire("b4", "condition2", "b5", "odd"),
            wire("b4", "default", "b5", "default"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert_eq!(summary.outputs["then"], "payload");
    assert!(!summary.outputs.contains_key("else"));
    // A Condition sends its variables.
    assert_eq!(summary.outputs["odd"], json!({"n": 7}));
    assert!(!summary.outputs.contains_key("default"));
}

#[test]
fn for_loops_collect_each_items_result_in_order() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::List {
                    items: vec!["3".to_owned(), "1".to_owned(), "2".to_owned()],
                },
            ),
            block("b2", evaluate(&["list"], "list")),
            block("b3", BlockType::For.block_kind()),
            block("b4", evaluate(&["item", "factor"], "item * factor")),
            block("b5", BlockKind::Number { value: 10. }),
            block("b6", BlockType::Collect.block_kind()),
            block("b7", output(&["list", "finished"])),
        ],
        connections: vec![
            wire("b1", "list", "b2", "list"),
            wire("b2", "result", "b3", "list"),
            wire("b3", "item", "b4", "item"),
            // From outside the loop, it is used by every iteration.
            wire("b5", "value", "b4", "factor"),
            wire("b4", "result", "b6", "item"),
            wire("b6", "list", "b7", "list"),
            wire("b6", "finish", "b7", "finished"),
        ],
    };

    let (summary, events) = run_flow(flow, options());

    assert_eq!(summary.outputs["list"], json!([30, 10, 20]));
    assert_eq!(summary.outputs["finished"], true);
    assert_eq!(finished(&events, "b4").len(), 3);
    assert_eq!(finished(&events, "b6").len(), 1);
}

#[test]
fn a_loop_iteration_combines_only_its_own_values() {
    // Two branches of each iteration meet again in one block.
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "[1, 2, 3]")),
            block("b2", BlockType::For.block_kind()),
            block("b3", evaluate(&["n"], "n * 10")),
            block("b4", evaluate(&["n"], "n * 100")),
            block("b5", evaluate(&["tens", "hundreds"], "tens + hundreds")),
            block("b6", BlockType::Collect.block_kind()),
            block("b7", output(&["sums"])),
        ],
        connections: vec![
            wire("b1", "result", "b2", "list"),
            wire("b2", "item", "b3", "n"),
            wire("b2", "item", "b4", "n"),
            wire("b3", "result", "b5", "tens"),
            wire("b4", "result", "b5", "hundreds"),
            wire("b5", "result", "b6", "item"),
            wire("b6", "list", "b7", "sums"),
        ],
    };

    let (summary, events) = run_flow(flow, options());

    assert_eq!(summary.outputs["sums"], json!([110, 220, 330]));
    assert_eq!(finished(&events, "b5").len(), 3);
}

#[test]
fn nested_loops_and_filtered_or_empty_loops_collect_correctly() {
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "[[1, 2], [], [3, 4, 5]]")),
            block("b2", BlockType::For.block_kind()),
            block("b3", BlockType::For.block_kind()),
            block(
                "b4",
                BlockKind::If {
                    variables: vec!["n".to_owned()],
                    condition: "n % 2 = 1".to_owned(),
                },
            ),
            block("b5", BlockType::Collect.block_kind()),
            block("b6", BlockType::Collect.block_kind()),
            block("b7", output(&["odds"])),
        ],
        connections: vec![
            wire("b1", "result", "b2", "list"),
            wire("b2", "item", "b3", "list"),
            wire("b3", "item", "b4", "n"),
            wire("b3", "item", "b4", "data"),
            wire("b4", "then", "b5", "item"),
            wire("b5", "list", "b6", "item"),
            wire("b6", "list", "b7", "odds"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert_eq!(summary.outputs["odds"], json!([[1], [], [3, 5]]));
}

#[test]
fn repeat_counts_and_an_empty_repeat_still_collects() {
    let flow = Flow {
        blocks: vec![
            block("b1", BlockKind::Number { value: 3. }),
            block("b2", BlockType::Repeat.block_kind()),
            block("b3", BlockType::Collect.block_kind()),
            block("b4", BlockKind::Number { value: 0. }),
            block("b5", BlockType::Repeat.block_kind()),
            block("b6", evaluate(&["i"], "i")),
            block("b7", BlockType::Collect.block_kind()),
            block("b8", output(&["indexes", "none"])),
        ],
        connections: vec![
            wire("b1", "value", "b2", "count"),
            wire("b2", "index", "b3", "item"),
            wire("b3", "list", "b8", "indexes"),
            wire("b4", "value", "b5", "count"),
            wire("b5", "index", "b6", "i"),
            wire("b6", "result", "b7", "item"),
            wire("b7", "list", "b8", "none"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert_eq!(summary.outputs["indexes"], json!([0, 1, 2]));
    assert_eq!(summary.outputs["none"], json!([]));
}

#[test]
fn variables_reach_every_get_variable_block() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::String {
                    value: "secret".to_owned(),
                },
            ),
            block(
                "b2",
                BlockKind::SetVariable {
                    name: "token".to_owned(),
                },
            ),
            block(
                "b3",
                BlockKind::GetVariable {
                    name: "token".to_owned(),
                },
            ),
            block("b4", evaluate(&["token"], "$uppercase(token)")),
            block("b5", output(&["token"])),
        ],
        connections: vec![
            wire("b1", "value", "b2", "value"),
            wire("b3", "value", "b4", "token"),
            wire("b4", "result", "b5", "token"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert_eq!(summary.outputs["token"], "SECRET");
}

#[test]
fn templates_records_lists_validation_and_delays() {
    let flow = Flow {
        blocks: vec![
            block("b1", BlockKind::String { value: "Ada".to_owned() }),
            block(
                "b2",
                BlockKind::Template {
                    variables: vec!["name".to_owned()],
                    template: "{\"greeting\": \"Hi {{name}}\"}".to_owned(),
                    format: TemplateFormat::Json,
                },
            ),
            block(
                "b3",
                BlockKind::Record {
                    fields: vec![
                        Field {
                            key: "name".to_owned(),
                            value: String::new(),
                        },
                        Field {
                            key: "age".to_owned(),
                            value: "36".to_owned(),
                        },
                        Field {
                            key: "note".to_owned(),
                            value: "plain text".to_owned(),
                        },
                    ],
                },
            ),
            block(
                "b4",
                BlockKind::Validate {
                    schema: r#"{"type": "object", "required": ["name", "age"], "properties": {"age": {"type": "integer"}}}"#.to_owned(),
                },
            ),
            block(
                "b5",
                BlockKind::Validate {
                    schema: r#"{"type": "string"}"#.to_owned(),
                },
            ),
            block("b6", BlockKind::Delay { milliseconds: 20 }),
            block("b7", output(&["template", "valid", "invalid", "delayed"])),
        ],
        connections: vec![
            wire("b1", "value", "b2", "name"),
            wire("b2", "result", "b7", "template"),
            wire("b1", "value", "b3", "name"),
            wire("b3", "record", "b4", "data"),
            wire("b4", "pass", "b7", "valid"),
            wire("b3", "record", "b5", "data"),
            wire("b5", "fail", "b7", "invalid"),
            wire("b1", "value", "b6", "data"),
            wire("b6", "data", "b7", "delayed"),
        ],
    };

    let started = std::time::Instant::now();
    let (summary, _) = run_flow(flow, options());

    assert!(started.elapsed() >= Duration::from_millis(20));
    assert_eq!(summary.outputs["template"], json!({"greeting": "Hi Ada"}));
    assert_eq!(
        summary.outputs["valid"],
        json!({"name": "Ada", "age": 36, "note": "plain text"})
    );
    assert_eq!(summary.outputs["invalid"]["errors"][0]["path"], "");
    assert_eq!(summary.outputs["delayed"], "Ada");
}

#[test]
fn failures_are_reported_and_the_rest_of_the_flow_runs() {
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "1 +")),
            block("b2", evaluate(&[], "$nothing")),
            block(
                "b3",
                BlockKind::String {
                    value: "still runs".to_owned(),
                },
            ),
            block("b4", output(&["text"])),
        ],
        connections: vec![wire("b3", "value", "b4", "text")],
    };

    let (summary, events) = run_flow(flow, options());

    assert_eq!(summary.failures, 1);
    assert!(finished(&events, "b1")[0].error.is_some());
    assert_eq!(
        finished(&events, "b2")[0].notice.as_deref(),
        Some("Result is undefined")
    );
    assert_eq!(summary.outputs["text"], "still runs");
}

#[test]
fn an_endless_cycle_stops_at_the_block_run_limit() {
    let flow = Flow {
        blocks: vec![
            block("b1", BlockKind::Number { value: 0. }),
            block("b2", BlockKind::Or),
            block("b3", evaluate(&["n"], "n + 1")),
        ],
        connections: vec![
            wire("b1", "value", "b2", "first"),
            wire("b2", "data", "b3", "n"),
            wire("b3", "result", "b2", "second"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert!(summary.stopped.unwrap().contains("Stopped after"));
    assert_eq!(summary.block_runs, crate::MAX_BLOCK_RUNS);
}

#[test]
fn logs_report_each_value() {
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "[\"a\", \"b\"]")),
            block("b2", BlockType::For.block_kind()),
            block("b3", BlockKind::Log),
        ],
        connections: vec![
            wire("b1", "result", "b2", "list"),
            wire("b2", "item", "b3", "data"),
        ],
    };

    let (_, events) = run_flow(flow, options());
    let logged: Vec<Value> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Log { value, .. } => Some((**value).clone()),
            _ => None,
        })
        .collect();

    assert_eq!(logged, [json!("a"), json!("b")]);
}

async fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }

    let head = String::from_utf8(head).unwrap();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();

    (head, body)
}

/// Answers every request with JSON describing it: its target and body, and
/// a next page until page 3. `/missing` answers 404.
async fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    smol::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            smol::spawn(async move {
                let (head, body) = read_request(&mut stream).await;
                let target = head.split(' ').nth(1).unwrap_or_default().to_owned();
                let page: u64 = target
                    .split("page=")
                    .nth(1)
                    .and_then(|page| page.parse().ok())
                    .unwrap_or(1);
                let status = if target.starts_with("/missing") {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                let json = json!({
                    "target": target,
                    "body": String::from_utf8_lossy(&body),
                    "page": page,
                    "next": (page < 3).then_some(page + 1),
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nX-Test: yes\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            })
            .detach();
        }
    })
    .detach();

    url
}

fn saved(name: &str, request: HttpRequest) -> SavedRequest {
    SavedRequest {
        name: name.to_owned(),
        request,
        collection: HashMap::from([("base".to_owned(), "unused".to_owned())]),
        environment: HashMap::new(),
        variables_error: None,
        scripts: Ok(Default::default()),
        session: Default::default(),
    }
}

#[test]
fn http_requests_fill_variables_from_inputs_and_route_by_status() {
    smol::block_on(async {
        let url = serve().await;
        let requests = HashMap::from([
            (
                "create".to_owned(),
                saved(
                    "Create",
                    HttpRequest {
                        method: Method::Post,
                        // A `{{send}}` variable is filled by the Send input.
                        path: format!("{url}/users?name={{{{name}}}}&mode={{{{send}}}}"),
                        body: Some(request::Body::json("{\"id\": {{id}}}")),
                        ..Default::default()
                    },
                ),
            ),
            (
                "missing".to_owned(),
                saved(
                    "Missing",
                    HttpRequest {
                        path: format!("{url}/missing"),
                        ..Default::default()
                    },
                ),
            ),
        ]);
        let flow = Flow {
            blocks: vec![
                block(
                    "b1",
                    BlockKind::String {
                        value: "Ada".to_owned(),
                    },
                ),
                block("b2", BlockKind::Number { value: 42. }),
                block(
                    "b3",
                    BlockKind::HttpRequest {
                        request: "create".to_owned(),
                    },
                ),
                block(
                    "b4",
                    BlockKind::HttpRequest {
                        request: "missing".to_owned(),
                    },
                ),
                block("b5", output(&["created", "missing"])),
                block(
                    "b6",
                    BlockKind::String {
                        value: "new".to_owned(),
                    },
                ),
            ],
            connections: vec![
                wire("b1", "value", "b3", "name"),
                wire("b6", "value", "b3", "send"),
                wire("b2", "value", "b3", "id"),
                wire("b3", "success", "b5", "created"),
                wire("b4", "fail", "b5", "missing"),
            ],
        };

        let mut events = Vec::new();
        let summary = run(
            flow,
            RunOptions {
                requests,
                ..options()
            },
            |event| events.push(event),
        )
        .await;

        let created = &summary.outputs["created"];
        assert_eq!(created["http"]["status"], 200);
        assert_eq!(created["http"]["headers"]["x-test"], "yes");
        assert_eq!(created["body"]["target"], "/users?name=Ada&mode=new");
        assert_eq!(created["body"]["body"], "{\"id\": 42}");
        assert_eq!(created["binary"], false);
        assert!(created["http"]["time"].as_f64().unwrap() >= 0.);

        assert_eq!(summary.outputs["missing"]["http"]["status"], 404);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RunEvent::Started { block } if block == "b3"))
        );
    });
}

#[test]
fn http_requests_wait_for_send_and_can_page_through_a_cycle() {
    smol::block_on(async {
        let url = serve().await;
        let requests = HashMap::from([(
            "page".to_owned(),
            saved(
                "Page",
                HttpRequest {
                    path: format!("{url}/items?page={{{{page}}}}"),
                    ..Default::default()
                },
            ),
        )]);
        // Start → OR → request(page) → If next exists → back to OR with the next page.
        let flow = Flow {
            blocks: vec![
                block(
                    "b1",
                    BlockKind::Start {
                        input: "1".to_owned(),
                    },
                ),
                block("b2", BlockKind::Or),
                block(
                    "b3",
                    BlockKind::HttpRequest {
                        request: "page".to_owned(),
                    },
                ),
                block(
                    "b4",
                    BlockKind::If {
                        variables: vec!["response".to_owned()],
                        condition: "response.body.next != null".to_owned(),
                    },
                ),
                block("b5", evaluate(&["response"], "response.body.next")),
                block("b6", BlockKind::Log),
                block("b7", evaluate(&["response"], "response.body.page")),
                block("b8", output(&["last"])),
            ],
            connections: vec![
                wire("b1", "data", "b2", "first"),
                wire("b2", "data", "b3", "page"),
                wire("b3", "success", "b4", "response"),
                wire("b3", "success", "b4", "data"),
                wire("b4", "then", "b5", "response"),
                wire("b5", "result", "b2", "second"),
                wire("b3", "success", "b6", "data"),
                wire("b4", "else", "b7", "response"),
                wire("b7", "result", "b8", "last"),
            ],
        };

        let mut events = Vec::new();
        let summary = run(
            flow,
            RunOptions {
                requests,
                ..options()
            },
            |event| events.push(event),
        )
        .await;

        let pages: Vec<Value> = events
            .iter()
            .filter_map(|event| match event {
                RunEvent::Log { value, .. } => Some(value["body"]["page"].clone()),
                _ => None,
            })
            .collect();
        assert_eq!(pages, [json!(1), json!(2), json!(3)]);
        assert_eq!(summary.outputs["last"], 3);
        assert_eq!(summary.failures, 0);
    });
}

#[test]
fn unknown_requests_and_bad_settings_fail_their_block() {
    let flow = Flow {
        blocks: vec![
            block(
                "b1",
                BlockKind::HttpRequest {
                    request: String::new(),
                },
            ),
            block(
                "b2",
                BlockKind::HttpRequest {
                    request: "gone".to_owned(),
                },
            ),
            block(
                "b3",
                BlockKind::Date {
                    value: "yesterday".to_owned(),
                },
            ),
            block(
                "b4",
                BlockKind::Date {
                    value: "2024-05-01".to_owned(),
                },
            ),
            block("b5", output(&["date"])),
        ],
        connections: vec![wire("b4", "value", "b5", "date")],
    };

    let (summary, events) = run_flow(flow, options());

    assert_eq!(summary.failures, 3);
    assert!(
        finished(&events, "b1")[0]
            .error
            .as_deref()
            .unwrap()
            .contains("Choose a request")
    );
    assert!(
        finished(&events, "b2")[0]
            .error
            .as_deref()
            .unwrap()
            .contains("no longer saved")
    );
    assert_eq!(summary.outputs["date"], 1714521600000_i64);
}

#[test]
fn an_inner_loop_runs_before_the_next_outer_iteration() {
    // Each inner item combines with a value of its own outer iteration.
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "[[1, 2], [3, 4]]")),
            block("b2", BlockType::For.block_kind()),
            block("b3", evaluate(&["group"], "$sum(group)")),
            block("b4", BlockType::For.block_kind()),
            block("b5", evaluate(&["n", "sum"], "n * sum")),
            block("b6", BlockType::Collect.block_kind()),
            block("b7", BlockType::Collect.block_kind()),
            block("b8", output(&["products"])),
        ],
        connections: vec![
            wire("b1", "result", "b2", "list"),
            wire("b2", "item", "b4", "list"),
            wire("b2", "item", "b3", "group"),
            wire("b4", "item", "b5", "n"),
            wire("b3", "result", "b5", "sum"),
            wire("b5", "result", "b6", "item"),
            wire("b6", "list", "b7", "item"),
            wire("b7", "list", "b8", "products"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert_eq!(summary.outputs["products"], json!([[3, 6], [21, 28]]));
}

#[test]
fn a_loop_inside_a_cycle_still_runs() {
    // For → OR → Repeat → back to OR: finding the loop's Collect ends.
    let flow = Flow {
        blocks: vec![
            block("b1", evaluate(&[], "[1]")),
            block("b2", BlockType::For.block_kind()),
            block("b3", BlockKind::Or),
            block("b4", BlockType::Repeat.block_kind()),
            block("b5", evaluate(&["i"], "i")),
        ],
        connections: vec![
            wire("b1", "result", "b2", "list"),
            wire("b2", "item", "b3", "first"),
            wire("b3", "data", "b4", "count"),
            wire("b4", "index", "b5", "i"),
            wire("b5", "result", "b3", "second"),
        ],
    };

    let (summary, _) = run_flow(flow, options());

    assert!(summary.block_runs > 0);
}
