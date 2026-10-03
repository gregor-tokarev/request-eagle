use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};

struct Cli(TempDir);

impl Cli {
    fn new() -> Self {
        let cli = Self(tempdir().unwrap());
        cli.call(json!({"command":"settings.proxy","mode":"disabled"}));
        cli
    }

    fn call(&self, input: Value) -> Value {
        let (code, value) = self.raw(&input.to_string());
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["ok"], true, "{value}");
        value["result"].clone()
    }

    fn raw(&self, input: &str) -> (i32, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
            .args(["--data-dir", self.0.path().to_str().unwrap(), "call", "-"])
            .env_remove("REQUEST_EAGLE_COLLECTIONS_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stdout).unwrap(),
        )
    }

    fn collection(&self) -> Value {
        self.call(json!({"command":"collections.create"}))["path"].clone()
    }
}

/// Answers each request with JSON naming its method, target and body, until
/// the test ends.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else {
                return;
            };
            thread::spawn(move || {
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut received = Vec::new();
                let mut buffer = [0; 4096];
                let (head, body) = loop {
                    let count = socket.read(&mut buffer).unwrap();
                    if count == 0 {
                        return;
                    }
                    received.extend_from_slice(&buffer[..count]);
                    let text = String::from_utf8_lossy(&received).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break (head.to_owned(), body.to_owned());
                        }
                    }
                };

                let mut parts = head.split(' ');
                let method = parts.next().unwrap_or_default();
                let target = parts.next().unwrap_or_default();
                let status = if target.starts_with("/missing") {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                let json = json!({"method": method, "target": target, "body": body}).to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
                let _ = socket.write_all(response.as_bytes());
            });
        }
    });

    url
}

#[test]
fn flows_are_created_listed_updated_moved_and_deleted() {
    let cli = Cli::new();
    let blocks = cli.call(json!({"command":"flows.blocks"}));
    assert_eq!(blocks["blocks"].as_array().unwrap().len(), 27);
    assert!(blocks["blocks"].as_array().unwrap().iter().any(|block| {
        block["type"] == "http_request" && block["outputs"] == json!(["success", "fail"])
    }));

    let collection = cli.collection();
    let folder = cli.call(json!({"command":"folders.create","parent":collection}))["path"].clone();
    let created = cli.call(json!({"command":"flows.create","parent":collection,"name":"Checkout"}));
    assert_eq!(created["name"], "Checkout");
    assert_eq!(created["flow"]["blocks"][0]["type"], "start");
    let path = created["path"].clone();

    // Flows are listed apart from requests.
    let listed = cli.call(json!({"command":"flows.list","query":"check"}));
    assert_eq!(listed[0]["id"], created["id"]);
    assert_eq!(listed[0]["blocks"], 1);
    assert_eq!(cli.call(json!({"command":"requests.list"})), json!([]));
    let tree = cli.call(json!({"command":"collections.get","path":collection}));
    assert!(
        tree["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "flow" && entry["id"] == created["id"])
    );

    let mut flow = created["flow"].clone();
    flow["blocks"].as_array_mut().unwrap().push(json!({
        "id": "b2", "type": "display", "x": 300, "y": 0, "title": "Show"
    }));
    flow["connections"] = json!([{"from":"b1","output":"data","to":"b2","input":"data"}]);
    let updated = cli.call(
        json!({"command":"flows.update","path":path,"expected_id":created["id"],"flow":flow}),
    );
    assert_eq!(updated["flow"]["blocks"][1]["title"], "Show");
    assert_eq!(updated["flow"]["connections"][0]["to"], "b2");

    // A connection to an input that does not exist is refused.
    let mut broken = flow.clone();
    broken["connections"][0]["input"] = json!("nope");
    let (code, error) = cli.raw(
        &json!({"command":"flows.update","path":path,"expected_id":created["id"],"flow":broken})
            .to_string(),
    );
    assert_eq!(code, 1, "{error}");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no input")
    );
    let (code, _) = cli.raw(
        &json!({"command":"flows.update","path":path,"expected_id":"another","flow":flow})
            .to_string(),
    );
    assert_eq!(code, 1);

    let renamed =
        cli.call(json!({"command":"entries.rename","path":path,"name":"Nightly checkout"}));
    let moved = cli.call(
        json!({"command":"entries.move","path":renamed["path"],"target":folder,"placement":"inside"}),
    );
    let read = cli.call(json!({"command":"flows.get","path":moved["path"]}));
    assert_eq!(read["name"], "Nightly checkout");
    assert_eq!(read["flow"], updated["flow"]);

    cli.call(json!({"command":"entries.delete","path":moved["path"],"confirm":true}));
    assert_eq!(cli.call(json!({"command":"flows.list"})), json!([]));
}

#[test]
fn runs_flows_that_send_requests_loop_and_return_outputs() {
    let cli = Cli::new();
    let url = serve();
    let collection = cli.collection();
    std::fs::write(
        std::path::Path::new(collection.as_str().unwrap()).join("environment.toml"),
        format!("base = {url:?}\n"),
    )
    .unwrap();
    let request = cli.call(
        json!({"command":"requests.create","parent":collection,"name":"Echo","request":{
            "method":"POST","url":"{{base}}/items/{{id}}","body":"{\"name\": \"{{name}}\"}"
        }}),
    );
    let missing = cli.call(
        json!({"command":"requests.create","parent":collection,"name":"Missing","request":{
            "method":"GET","url":"{{base}}/missing"
        }}),
    );

    let flow = json!({
        "blocks": [
            {"id":"b1","type":"start","x":0,"y":0},
            {"id":"b2","type":"select","x":0,"y":0,"path":"ids"},
            {"id":"b3","type":"for","x":0,"y":0},
            {"id":"b4","type":"select","x":0,"y":0,"path":"name"},
            {"id":"b5","type":"http_request","x":0,"y":0,"request":request["id"]},
            {"id":"b6","type":"evaluate","x":0,"y":0,"variables":["response"],"expression":"response.body.target & ' ' & response.body.body"},
            {"id":"b7","type":"collect","x":0,"y":0},
            {"id":"b8","type":"http_request","x":0,"y":0,"request":missing["id"]},
            {"id":"b9","type":"output","x":0,"y":0,"names":["sent","status"]},
            {"id":"b10","type":"select","x":0,"y":0,"path":"http.status"},
            {"id":"b11","type":"log","x":0,"y":0}
        ],
        "connections": [
            {"from":"b1","output":"data","to":"b2","input":"data"},
            {"from":"b2","output":"value","to":"b3","input":"list"},
            {"from":"b3","output":"item","to":"b5","input":"id"},
            {"from":"b1","output":"data","to":"b4","input":"data"},
            {"from":"b4","output":"value","to":"b5","input":"name"},
            {"from":"b5","output":"success","to":"b6","input":"response"},
            {"from":"b6","output":"result","to":"b7","input":"item"},
            {"from":"b7","output":"list","to":"b9","input":"sent"},
            {"from":"b8","output":"fail","to":"b10","input":"data"},
            {"from":"b10","output":"value","to":"b9","input":"status"},
            {"from":"b3","output":"item","to":"b11","input":"data"}
        ]
    });
    let created = cli.call(
        json!({"command":"flows.create","parent":collection,"name":"Send items","flow":flow}),
    );
    let got = cli.call(json!({"command":"flows.get","path":created["path"]}));
    assert_eq!(
        got["requests"][request["id"].as_str().unwrap()]["variables"],
        json!(["base", "id", "name"])
    );

    let run = cli.call(json!({"command":"flows.run","path":created["path"],
        "input":{"ids":[7, 8], "name":"Ada"}}));

    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(
        run["outputs"]["sent"],
        json!([
            "/items/7 {\"name\": \"Ada\"}",
            "/items/8 {\"name\": \"Ada\"}"
        ])
    );
    assert_eq!(run["outputs"]["status"], 404);
    assert_eq!(run["failures"], 0);
    let block = |id: &str| {
        run["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(block("b5")["runs"], 2);
    assert_eq!(block("b5")["outputs"]["success"]["http"]["status"], 200);
    assert_eq!(block("b3")["notice"], "Sent 2 items");
    assert_eq!(
        run["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|log| log["value"].clone())
            .collect::<Vec<_>>(),
        [json!(7), json!(8)]
    );

    // Variables passed to the run override the collection's. Requests that
    // cannot connect go out of Fail, so the loop collects nothing.
    let unreachable = cli.call(json!({"command":"flows.run","path":created["path"],
        "input":{"ids":[1], "name":"Bo"}, "variables":{"base":"http://127.0.0.1:9"}}));
    assert_eq!(unreachable["status"], "succeeded", "{unreachable}");
    assert_eq!(unreachable["outputs"]["sent"], json!([]));
    assert_eq!(unreachable["outputs"]["status"], Value::Null);
}

#[test]
fn flow_runs_need_trust_for_scripts_and_stop_at_their_timeout() {
    let cli = Cli::new();
    let collection = cli.collection();
    let scripted = cli.call(
        json!({"command":"requests.create","parent":collection,"name":"Scripted","request":{
            "method":"GET","url":"http://127.0.0.1:9","pre_request":"pm.variables.set('a', 1)"
        }}),
    );
    let flow = cli.call(
        json!({"command":"flows.create","parent":collection,"name":"Scripted","flow":{
            "blocks":[{"id":"b1","type":"http_request","x":0,"y":0,"request":scripted["id"]}]
        }}),
    );
    let (code, error) = cli.raw(&json!({"command":"flows.run","path":flow["path"]}).to_string());
    assert_eq!(code, 1);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trust_scripts")
    );

    let slow = cli.call(
        json!({"command":"flows.create","parent":collection,"name":"Slow","flow":{
            "blocks":[
                {"id":"b1","type":"start","x":0,"y":0},
                {"id":"b2","type":"delay","x":0,"y":0,"milliseconds":20000}
            ],
            "connections":[{"from":"b1","output":"data","to":"b2","input":"data"}]
        }}),
    );
    let started = Instant::now();
    let run = cli.call(json!({"command":"flows.run","path":slow["path"],"timeout_ms":300}));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(run["status"], "stopped");
    assert!(run["stopped"].as_str().unwrap().contains("timeout_ms"));
}

#[test]
fn evaluates_fql_like_evaluate_blocks() {
    let cli = Cli::new();

    let result = cli.call(json!({"command":"fql.evaluate",
        "expression":"$sum(orders.(price * quantity)) & ' ' & $currency",
        "input":{"orders":[{"price":2,"quantity":3},{"price":1.5,"quantity":2}]},
        "bindings":{"currency":"EUR"}}));
    assert_eq!(result, json!({"defined": true, "result": "9 EUR"}));

    let missing =
        cli.call(json!({"command":"fql.evaluate","expression":"nothing.here","input":{}}));
    assert_eq!(missing, json!({"defined": false, "result": null}));

    let (code, error) = cli.raw(&json!({"command":"fql.evaluate","expression":"1 +"}).to_string());
    assert_eq!(code, 1);
    assert_eq!(error["error"]["code"], "operation_failed");
}
