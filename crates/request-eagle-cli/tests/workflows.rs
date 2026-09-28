use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use tempfile::{TempDir, tempdir};

struct Cli(TempDir);

impl Cli {
    fn new() -> Self {
        Self(tempdir().unwrap())
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
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
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

    fn create(&self, parent: &Value, request: Value) -> Value {
        self.call(json!({"command":"requests.create", "parent":parent,"name":"Example","request":request}))
    }
}

#[test]
fn schema_describes_only_saved_data_operations() {
    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .arg("schema")
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema = String::from_utf8(output.stdout).unwrap();
    assert!(schema.contains("requests.update") && schema.contains("requests.run"));
    for removed in [
        "tabs.",
        "drafts.",
        "responses.get",
        "ui.",
        "socket",
        "session_token",
    ] {
        assert!(!schema.contains(removed), "{removed}");
    }
}

#[test]
fn saved_collection_lifecycle_uses_the_backend_without_an_app() {
    let cli = Cli::new();
    assert_eq!(cli.call(json!({"command":"collections.list"})), json!([]));
    let collection = cli.collection();
    let folder = cli.call(json!({"command":"folders.create","parent":collection}))["path"].clone();
    let created = cli.create(
        &folder,
        json!({"method":"POST","url":"https://example.invalid","body":"old"}),
    );
    let path = created["path"].as_str().unwrap();
    let source = fs::read_to_string(path).unwrap();
    fs::write(
        path,
        format!("# preserved comment\n{source}\ncustom_metadata = \"keep me\"\n"),
    )
    .unwrap();

    let mut request = created["request"].clone();
    request["body"] = json!([0, 255, 128]);
    request["headers"] = json!([["X-Test", "one"], ["X-Test", "two"]]);
    let updated = cli.call(json!({"command":"requests.update","path":path,"expected_id":created["id"],"request":request}));
    assert_eq!(updated["id"], created["id"]);
    assert_eq!(updated["request"], request);
    let source = fs::read_to_string(path).unwrap();
    assert!(source.contains("# preserved comment") && source.contains("custom_metadata"));

    let tree = cli.call(json!({"command":"collections.get","path":collection}));
    assert_eq!(tree["entries"][0]["entries"][0]["id"], created["id"]);
    let renamed =
        cli.call(json!({"command":"entries.rename","path":folder,"name":"Renamed"}))["path"]
            .clone();
    let listed =
        cli.call(json!({"command":"requests.list","collection":collection,"query":"example"}));
    let moved_path = listed[0]["path"].clone();
    assert!(
        moved_path
            .as_str()
            .unwrap()
            .starts_with(renamed.as_str().unwrap())
    );
    let moved = cli.call(json!({"command":"entries.move","path":moved_path,"target":collection,"placement":"inside"}));
    let value = cli.call(json!({"command":"requests.get","path":moved["path"]}));
    assert_eq!(value["request"], request);
    let (code, denied) = cli
        .raw(&json!({"command":"entries.delete","path":moved["path"],"confirm":false}).to_string());
    assert_eq!(code, 1);
    assert_eq!(denied["ok"], false);
    assert!(Path::new(moved["path"].as_str().unwrap()).exists());
    cli.call(json!({"command":"entries.delete","path":moved["path"],"confirm":true}));
    assert_eq!(cli.call(json!({"command":"requests.list"})), json!([]));
    cli.call(json!({"command":"entries.delete","path":collection,"confirm":true}));
    assert_eq!(cli.call(json!({"command":"collections.list"})), json!([]));
}

#[test]
fn edits_reject_replaced_requests_and_paths_outside_the_collection_root() {
    let cli = Cli::new();
    let collection = cli.collection();
    let created = cli.create(
        &collection,
        json!({"method":"GET","url":"https://example.invalid"}),
    );
    let path = created["path"].as_str().unwrap();
    let original = fs::read(path).unwrap();
    let (code, _) = cli.raw(&json!({"command":"requests.update","path":path,"expected_id":"wrong-id","request":created["request"]}).to_string());
    assert_eq!(code, 1);
    assert_eq!(fs::read(path).unwrap(), original);
    let outside = cli.0.path().join("outside.toml");
    fs::write(&outside, &original).unwrap();
    let (code, _) =
        cli.raw(&json!({"command":"entries.delete","path":outside,"confirm":true}).to_string());
    assert_eq!(code, 1);
    assert_eq!(fs::read(outside).unwrap(), original);
}

#[test]
fn runs_saved_requests_with_variables_scripts_and_lossless_responses() {
    let cli = Cli::new();
    cli.call(json!({"command":"settings.proxy","mode":"disabled"}));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut received = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = socket.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            received.extend_from_slice(&buffer[..count]);
            if received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(received).unwrap().to_lowercase();
        assert!(request.starts_with("get /from-script?value=override "));
        assert!(request.contains("x-agent: headless"));
        socket.write_all(b"HTTP/1.1 418 Teapot\r\nContent-Length: 4\r\nX-Repeat: one\r\nX-Repeat: two\r\nConnection: close\r\n\r\n\x00\xff\x80A").unwrap();
    });
    let collection = cli.collection();
    let environment = Path::new(collection.as_str().unwrap()).join("environment.toml");
    fs::write(&environment, format!("base = {url:?}\nvalue = \"saved\"\n")).unwrap();
    let created = cli.create(&collection, json!({
        "method":"GET", "url":"{{base}}/{{route}}", "query":[["value","{{value}}"]],
        "pre_request":"pm.environment.set('route', 'from-script'); pm.request.headers.add({key:'X-Agent', value:'headless'}); console.log('running');",
        "post_response":"pm.test('status', () => pm.response.to.have.status(418)); pm.test('failure', () => pm.expect(1).to.equal(2));"
    }));
    let (code, _) = cli.raw(&json!({"command":"requests.run","path":created["path"]}).to_string());
    assert_eq!(code, 1);
    let response = cli.call(json!({"command":"requests.run","path":created["path"],"trust_scripts":true,"variables":{"value":"override"},"timeout_ms":5000}));
    server.join().unwrap();
    assert_eq!(response["status"], 418);
    assert_eq!(
        STANDARD
            .decode(response["body_base64"].as_str().unwrap())
            .unwrap(),
        [0, 255, 128, b'A']
    );
    assert_eq!(
        response["headers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|header| header["name"] == "x-repeat")
            .count(),
        2
    );
    assert_eq!(response["scripts"][1]["tests"][0]["passed"], true);
    assert_eq!(response["scripts"][1]["tests"][1]["passed"], false);
    assert!(!fs::read_to_string(environment).unwrap().contains("route"));
}

#[test]
fn settings_patch_preserves_other_fields_and_rejects_invalid_input() {
    let cli = Cli::new();
    cli.call(json!({"command":"settings.request","timeout_ms":1200,"follow_all_redirects":false}));
    let updated =
        cli.call(json!({"command":"settings.appearance","mode":"light","interface_font_size":20}));
    assert_eq!(updated["request"]["timeout_ms"], 1200);
    assert_eq!(updated["appearance"]["mode"], "light");
    assert!(updated.get("proxy_credentials_id").is_none());
    let path = cli.0.path().join("preferences.json");
    let before = fs::read(&path).unwrap();
    for input in [
        json!({"command":"settings.appearance","interface_font_size":999}),
        json!({"command":"settings.proxy","host":"https://user:secret@proxy.invalid"}),
        json!({"command":"settings.proxy","password":"secret"}),
    ] {
        assert_eq!(cli.raw(&input.to_string()).0, 1);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    assert_eq!(
        cli.raw(r#"{"command":"settings.request","timeout_ms":1,"typo":true}"#)
            .0,
        2
    );
    assert_eq!(
        cli.raw(r#"{"command":"requests.run","path":"missing","trust_script":true}"#)
            .0,
        2
    );
    fs::write(&path, "malformed").unwrap();
    assert_eq!(
        cli.raw(r#"{"command":"settings.request","timeout_ms":5}"#)
            .0,
        1
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "malformed");
}

#[test]
fn settings_reads_do_not_reveal_legacy_proxy_secrets_or_erase_them() {
    let cli = Cli::new();
    let path = cli.0.path().join("preferences.json");
    let original =
        r#"{"request":{"proxy":{"username":"private-user","password":"private-password"}}}"#;
    fs::write(&path, original).unwrap();
    let result = cli.call(json!({"command":"settings.get"}));
    assert!(!result.to_string().contains("private-"));
    assert_eq!(
        cli.raw(r#"{"command":"settings.request","timeout_ms":5}"#)
            .0,
        1
    );
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

// The macOS runner's filesystem rejects invalid UTF-8 names before CLI startup.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_paths_fail_before_collection_mutation() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let cli = Cli::new();
    let root = cli.0.path().join("collections");
    fs::create_dir_all(root.join(OsString::from_vec(b"bad-\xff".to_vec()))).unwrap();
    let (code, response) = cli.raw(r#"{"command":"collections.create"}"#);
    assert_eq!(code, 1);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("UTF-8")
    );
    assert!(!root.join("New Collection").exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_return_a_structured_error() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .arg(OsString::from_vec(b"bad-\xff".to_vec()))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error"]["code"], "invalid_input");
}

#[test]
fn explicit_collection_directory_overrides_the_environment() {
    let cli = Cli::new();
    let selected = cli.0.path().join("selected");
    let ignored = cli.0.path().join("ignored");
    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .args([
            "--data-dir",
            cli.0.path().to_str().unwrap(),
            "--collections-dir",
            selected.to_str().unwrap(),
            "call",
            r#"{"command":"collections.create"}"#,
        ])
        .env("REQUEST_EAGLE_COLLECTIONS_DIR", &ignored)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(selected.join("New Collection").is_dir());
    assert!(!ignored.exists());
}
