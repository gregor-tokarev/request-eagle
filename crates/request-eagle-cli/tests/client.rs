use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::Command,
    thread,
};

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn schema_is_offline_and_invalid_inputs_are_structured() {
    let output = cli(&["schema"]);
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["ok"], true);
    assert!(
        schema["result"]["commands"]
            .to_string()
            .contains("drafts.set")
    );
    for input in [
        r#"{"command":"tabs.list","typo":true}"#,
        r#"{"command":"unknown"}"#,
    ] {
        let output = cli(&["call", input]);
        assert_eq!(output.status.code(), Some(2));
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "invalid_input");
    }
}

#[test]
fn sends_versioned_command_and_propagates_application_errors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("test.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            request,
            json!({"version":1,"command":{"command":"tabs.close","tab":7,"discard":false}})
        );
        writeln!(stream, "{}", json!({"version":1,"ok":false,"error":{"code":"operation_failed","message":"Unsaved draft"}})).unwrap();
    });
    let output = cli(&[
        "--socket",
        path.to_str().unwrap(),
        "call",
        r#"{"command":"tabs.close","tab":7}"#,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["message"],
        "Unsaved draft"
    );
    server.join().unwrap();
}

#[test]
fn refuses_socket_in_shared_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.path().join("test.sock");
    let _listener = UnixListener::bind(&path).unwrap();
    let output = cli(&[
        "--socket",
        path.to_str().unwrap(),
        "call",
        r#"{"command":"tabs.list"}"#,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout).unwrap().contains("0700"));
}

#[test]
fn times_out_without_retrying_a_mutation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("test.sock");
    let _listener = UnixListener::bind(&path).unwrap();
    let output = cli(&[
        "--socket",
        path.to_str().unwrap(),
        "--timeout-ms",
        "20",
        "call",
        r#"{"command":"tabs.new"}"#,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
        "connection_error"
    );
}
