use serde_json::{Value, json};
use std::{
    io::Read,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::Command,
    thread,
};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .env("REQUEST_EAGLE_CLI_TOKEN", TOKEN)
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
    let listener = smol::net::unix::UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        smol::block_on(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut connection = request_eagle_automation::Connection::server(stream, TOKEN)
                .await
                .unwrap();
            let bytes = connection.receive().await.unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                request,
                json!({"version":1,"command":{"command":"tabs.close","tab":7,"discard":false}})
            );
            let reply = serde_json::to_vec(&json!({"version":1,"ok":false,"error":{"code":"operation_failed","message":"Unsaved draft"}})).unwrap();
            connection.send(&reply).await.unwrap();
        })
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

#[test]
fn a_slow_instance_cannot_redirect_a_command_to_another_workspace() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let responsive = UnixListener::bind(dir.path().join("responsive.sock")).unwrap();
    let _slow = UnixListener::bind(dir.path().join("slow.sock")).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = responsive.accept().unwrap();
        let mut data = Vec::new();
        stream.read_to_end(&mut data).unwrap();
        assert!(
            data.is_empty(),
            "Discovery must not send credentials or commands"
        );
    });

    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .env("REQUEST_EAGLE_AUTOMATION_DIR", dir.path())
        .env("REQUEST_EAGLE_CLI_TOKEN", TOKEN)
        .args(["call", r#"{"command":"tabs.new"}"#])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("found 2")
    );
    server.join().unwrap();
}

#[test]
fn non_utf8_arguments_and_socket_paths_return_json_errors() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
        "invalid_input"
    );

    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(OsString::from_vec(vec![b'a', 0xff]));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _listener = UnixListener::bind(dir.join("test.sock")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
        .env("REQUEST_EAGLE_AUTOMATION_DIR", dir)
        .arg("instances")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("UTF-8")
    );
}

#[test]
fn counterfeit_listener_never_receives_credentials_or_command_payload() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("fake.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut length = [0u8; 2];
        stream.read_exact(&mut length).unwrap();
        let mut handshake = vec![0; u16::from_be_bytes(length) as usize];
        stream.read_exact(&mut handshake).unwrap();
        assert_eq!(handshake.len(), 48); // ephemeral public key and authentication tag
        assert!(
            !handshake
                .windows(16)
                .any(|bytes| bytes == &TOKEN.as_bytes()[..16])
        );
        // An impostor cannot produce the responder's proof. Closing the stream
        // must fail authentication before the CLI sends the requested mutation.
    });
    let output = cli(&[
        "--socket",
        path.to_str().unwrap(),
        "call",
        r#"{"command":"tabs.new"}"#,
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
        "unauthorized"
    );
    server.join().unwrap();
}
