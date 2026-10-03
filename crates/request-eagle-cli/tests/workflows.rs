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
fn every_unreadable_file_is_reported_with_its_error() {
    let cli = Cli::new();
    let [first, second] = [cli.collection(), cli.collection()]
        .map(|path| Path::new(path.as_str().unwrap()).to_path_buf());
    let request = first.join("Broken.toml");
    let environment = second.join("environment.toml");
    fs::write(&request, "id = ").unwrap();
    fs::write(&environment, "<<<<<<< HEAD\n").unwrap();

    let (code, output) = cli.raw(&json!({"command":"collections.list"}).to_string());

    assert_eq!(code, 1, "{output}");
    let message = output["error"]["message"].as_str().unwrap();
    for file in [&request, &environment] {
        let name = file.strip_prefix(file.parent().unwrap().parent().unwrap());
        assert!(
            message.contains(name.unwrap().to_str().unwrap()),
            "{message}"
        );
    }
    assert!(message.contains("line 1"), "{message}");
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
    // Text alone is raw JSON.
    assert_eq!(
        request["body"],
        json!({"type":"raw","language":"json","text":"old"})
    );
    request["body"] = json!({"type":"multipart","parts":[
        {"name":"title","value":"Hi"},
        {"name":"avatar","value":"files/eagle.png","file":true},
    ]});
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
fn collection_scripts_are_listed_and_need_trust_to_run() {
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
        while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = socket.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            received.extend_from_slice(&buffer[..count]);
        }
        assert!(
            String::from_utf8(received)
                .unwrap()
                .to_lowercase()
                .contains("x-collection: shared")
        );
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let collection = cli.collection();
    let directory = Path::new(collection.as_str().unwrap());
    fs::write(
        directory.join("environment.toml"),
        format!("base = {url:?}\n"),
    )
    .unwrap();
    fs::write(
        directory.join(".request-eagle-collection.toml"),
        "[scripts]\npre_request = \"pm.request.headers.add({key: 'X-Collection', value: 'shared'});\"\n",
    )
    .unwrap();
    let created = cli.create(&collection, json!({"method":"GET", "url":"{{base}}/plain"}));

    let details = cli.call(json!({"command":"collections.get","path":collection}));
    assert!(
        details["scripts"]["pre_request"]
            .as_str()
            .unwrap()
            .contains("X-Collection")
    );
    assert_eq!(details["variables"]["base"], url);
    // The settings file is not listed as a request.
    assert_eq!(details["entries"].as_array().unwrap().len(), 1);

    let (code, _) = cli.raw(&json!({"command":"requests.run","path":created["path"]}).to_string());
    assert_eq!(code, 1);
    let response = cli.call(json!({"command":"requests.run","path":created["path"],"trust_scripts":true,"timeout_ms":5000}));
    server.join().unwrap();
    assert_eq!(response["status"], 204);
    assert_eq!(response["scripts"][0]["collection"], true);
}

#[test]
fn runs_share_the_cookie_jar_and_cookie_commands_manage_it() {
    let cli = Cli::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut heads = Vec::new();

        for _ in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0; 1024];
            while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                received.extend_from_slice(&buffer[..count]);
            }
            heads.push(String::from_utf8(received).unwrap());
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nSet-Cookie: session=abc; Max-Age=3600\r\nConnection: close\r\n\r\n")
                .unwrap();
        }

        heads
    });
    let collection = cli.collection();
    let created = cli.create(&collection, json!({"method":"GET","url":url}));
    let run = json!({"command":"requests.run","path":created["path"],"timeout_ms":5000});

    cli.call(run.clone());
    cli.call(run.clone());
    let cookies = cli.call(json!({"command":"cookies.list","domain":"127.0.0.1"}));
    assert_eq!(cookies.as_array().unwrap().len(), 1);
    assert_eq!(cookies[0]["name"], "session");
    assert_eq!(cookies[0]["value"], "abc");
    assert!(cookies[0]["expires"].is_u64());

    let deleted = cli.call(json!({"command":"cookies.delete","domain":"127.0.0.1"}));
    assert_eq!(deleted["deleted"][0]["name"], "session");
    assert_eq!(cli.call(json!({"command":"cookies.list"})), json!([]));

    let settings = cli.call(json!({"command":"settings.request","cookie_jar":false}));
    assert_eq!(settings["request"]["cookie_jar"], false);
    cli.call(run);
    assert_eq!(cli.call(json!({"command":"cookies.list"})), json!([]));

    let heads = server.join().unwrap();
    assert!(!heads[0].contains("\r\ncookie:"));
    assert!(heads[1].contains("\r\ncookie: session=abc\r\n"));
    assert!(!heads[2].contains("\r\ncookie:"));
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
fn request_settings_are_saved_with_the_request() {
    let cli = Cli::new();
    let collection = cli.collection();
    let created = cli.create(
        &collection,
        json!({"method":"GET","url":"https://example.invalid","timeout_ms":0,"follow_redirects":false,"verify_certificates":false,"send_cookies":false}),
    );
    let path = created["path"].as_str().unwrap();
    assert_eq!(created["request"]["timeout_ms"], 0);
    assert_eq!(created["request"]["follow_redirects"], false);
    assert_eq!(created["request"]["verify_certificates"], false);
    assert_eq!(created["request"]["send_cookies"], false);
    let source = fs::read_to_string(path).unwrap();
    assert!(source.contains("[request.settings]"), "{source}");
    assert!(source.contains("send_cookies = false"), "{source}");

    // Settings left unset follow the preferences and leave the file.
    let mut request = created["request"].clone();
    request.as_object_mut().unwrap().remove("timeout_ms");
    request
        .as_object_mut()
        .unwrap()
        .remove("verify_certificates");
    request.as_object_mut().unwrap().remove("send_cookies");
    let updated = cli.call(json!({"command":"requests.update","path":path,"expected_id":created["id"],"request":request}));
    assert_eq!(updated["request"], request);
    let source = fs::read_to_string(path).unwrap();
    assert!(source.contains("follow_redirects = false"), "{source}");
    assert!(
        !source.contains("timeout_ms")
            && !source.contains("verify_certificates")
            && !source.contains("send_cookies"),
        "{source}"
    );
}

#[test]
fn certificate_settings_are_added_and_removed() {
    let cli = Cli::new();
    let ca = cli.0.path().join("ca.pem");
    let settings = cli.call(json!({"command":"settings.request","ca_certificates":ca}));
    assert_eq!(settings["request"]["ca_certificates"], json!(ca));

    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["api.example.com".into()]).unwrap();
    let certificate_path = cli.0.path().join("client.crt");
    let key_path = cli.0.path().join("client.key");
    let combined_path = cli.0.path().join("client.pem");
    fs::write(&certificate_path, cert.pem()).unwrap();
    fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    fs::write(&combined_path, cert.pem() + &signing_key.serialize_pem()).unwrap();

    let settings = cli.call(json!({"command":"settings.client_certificates.add","host":" api.example.com:8443 ","certificate":certificate_path,"key":key_path}));
    let certificate = &settings["request"]["client_certificates"][0];
    assert_eq!(certificate["host"], "api.example.com:8443");
    assert_eq!(
        certificate["files"],
        json!({"format":"pem","certificate":certificate_path,"key":key_path})
    );
    let id = certificate["id"].as_str().unwrap().to_owned();
    cli.call(json!({"command":"settings.client_certificates.add","host":"*.example.com","certificate":combined_path}));

    let path = cli.0.path().join("preferences.json");
    let before = fs::read(&path).unwrap();
    for input in [
        json!({"command":"settings.client_certificates.add","host":"https://api.example.com","certificate":combined_path}),
        json!({"command":"settings.client_certificates.add","host":"api.example.com"}),
        json!({"command":"settings.client_certificates.add","host":"api.example.com","certificate":combined_path,"pkcs12":"/certs/client.p12"}),
        json!({"command":"settings.client_certificates.add","host":"api.example.com","pkcs12":"/certs/missing.p12"}),
        json!({"command":"settings.client_certificates.add","host":"api.example.com","certificate":certificate_path}),
        json!({"command":"settings.client_certificates.remove","id":"unknown"}),
    ] {
        assert_eq!(cli.raw(&input.to_string()).0, 1, "{input}");
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    let settings = cli.call(json!({"command":"settings.client_certificates.remove","id":id}));
    let certificates = settings["request"]["client_certificates"]
        .as_array()
        .unwrap();
    assert_eq!(certificates.len(), 1);
    assert_eq!(certificates[0]["host"], "*.example.com");

    let settings = cli.call(json!({"command":"settings.request","ca_certificates":""}));
    assert!(settings["request"].get("ca_certificates").is_none());
}

#[test]
fn settings_edits_preserve_the_desktop_vim_preference() {
    let cli = Cli::new();
    let path = cli.0.path().join("preferences.json");

    for enabled in [true, false] {
        fs::write(&path, json!({"vim_mode": enabled}).to_string()).unwrap();

        for command in [
            json!({"command":"settings.request","timeout_ms":1200}),
            json!({"command":"settings.appearance","mode":"light"}),
            json!({"command":"settings.proxy","mode":"disabled"}),
        ] {
            cli.call(command);

            let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["vim_mode"], enabled);
            let reloaded = preferences::PreferencesFile::new(cli.0.path())
                .read()
                .unwrap();
            assert_eq!(reloaded.vim_mode, enabled);
        }
    }
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

#[cfg(unix)]
#[test]
fn read_only_collections_can_be_listed_read_and_run_without_writing_a_lock() {
    use std::os::unix::fs::PermissionsExt;

    let cli = Cli::new();
    cli.call(json!({"command":"settings.proxy","mode":"disabled"}));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0; 1024];
            while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                received.extend_from_slice(&buffer[..count]);
            }
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .unwrap();
        }
    });
    let collection = cli.collection();
    let created = cli.create(&collection, json!({"method":"GET","url":url}));
    let root = cli.0.path().join("collections");
    let lock_path = root.join(".cli.lock");
    fs::remove_file(&lock_path).unwrap();

    for existing_lock in [false, true] {
        if existing_lock {
            fs::write(&lock_path, "existing lock").unwrap();
            fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o444)).unwrap();
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
        let results = [
            json!({"command":"collections.list"}),
            json!({"command":"collections.get","path":collection}),
            json!({"command":"requests.list"}),
            json!({"command":"requests.get","path":created["path"]}),
            json!({"command":"requests.run","path":created["path"],"timeout_ms":3000}),
        ]
        .map(|command| cli.raw(&command.to_string()));
        let lock_exists = lock_path.exists();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        for (code, result) in &results {
            assert_eq!(*code, 0, "{result}");
            assert_eq!(result["ok"], true);
        }
        assert_eq!(results[4].1["result"]["status"], 204);
        assert_eq!(lock_exists, existing_lock);
        if existing_lock {
            assert_eq!(fs::read_to_string(&lock_path).unwrap(), "existing lock");
        }
    }
    server.join().unwrap();
}

#[test]
fn collection_edits_still_require_the_exclusive_lock() {
    let cli = Cli::new();
    let collection = cli.collection();
    let root = cli.0.path().join("collections");
    let lock = fs::OpenOptions::new()
        .append(true)
        .open(root.join(".cli.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let (code, result) = cli.raw(r#"{"command":"collections.create"}"#);
    assert_eq!(code, 1);
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("retry")
    );
    let listed = cli.call(json!({"command":"collections.list"}));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["path"], collection);
}

#[test]
fn grpc_requests_are_saved_and_listed_with_their_protocol() {
    let cli = Cli::new();
    let collection = cli.collection();
    let grpc = json!({
        "protocol": "grpc",
        "url": "grpcs://example.invalid",
        "tls": true,
        "method": "echo.v1.EchoService/Say",
        "message": "{\"text\": \"hi\"}",
        "metadata": [["authorization", "Bearer {{token}}"]],
        "proto_file": "protos/echo.proto",
        "import_paths": ["shared"],
    });
    let created = cli.create(&collection, grpc.clone());
    let path = created["path"].as_str().unwrap();
    assert_eq!(created["request"], grpc);

    let source = fs::read_to_string(path).unwrap();
    assert!(source.contains("type = \"grpc\""), "{source}");
    assert!(source.contains("source = \"proto_file\""), "{source}");

    let listed = cli.call(json!({"command":"requests.list","query":"EchoService"}));
    assert_eq!(listed[0]["protocol"], "grpc");
    assert_eq!(listed[0]["url"], "grpcs://example.invalid");

    // Changing protocol leaves no gRPC fields behind, and back again.
    let http = json!({"method":"GET","url":"https://example.invalid","headers":[],"query":[],"body":null,"pre_request":"","post_response":""});
    let updated = cli.call(
        json!({"command":"requests.update","path":path,"expected_id":created["id"],"request":http}),
    );
    assert_eq!(updated["request"], http);
    let source = fs::read_to_string(path).unwrap();
    assert!(
        !source.contains("proto_file") && !source.contains("metadata"),
        "{source}"
    );

    let reflection = json!({"protocol":"grpc","url":"localhost:50051","tls":false,"method":"","message":"","metadata":[]});
    let updated = cli.call(json!({"command":"requests.update","path":path,"expected_id":created["id"],"request":reflection}));
    assert_eq!(updated["request"], reflection);
    let source = fs::read_to_string(path).unwrap();
    assert!(
        !source.contains("GET") && !source.contains("definition"),
        "{source}"
    );
}

#[test]
fn grpc_scripts_are_saved_and_need_approval_to_run() {
    let cli = Cli::new();
    let collection = cli.collection();
    let grpc = json!({
        "protocol": "grpc",
        "url": "127.0.0.1:1",
        "tls": false,
        "method": "echo.v1.EchoService/Say",
        "message": "{}",
        "metadata": [],
        "before_invoke": "pm.request.metadata.add({key: 'x-id', value: '1'});",
        "after_response": "pm.test('ok', () => pm.response.to.be.ok);",
    });
    let created = cli.create(&collection, grpc.clone());
    assert_eq!(created["request"], grpc);

    let source = fs::read_to_string(created["path"].as_str().unwrap()).unwrap();
    assert!(source.contains("before_invoke"), "{source}");
    assert!(!source.contains("on_message"), "{source}");

    let (_, denied) =
        cli.raw(&json!({"command":"requests.run","path":created["path"]}).to_string());
    assert_eq!(denied["ok"], false);
    assert!(
        denied["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trust_scripts"),
        "{denied}"
    );
}

#[test]
fn authorizations_are_saved_and_runs_send_the_collections() {
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
        while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = socket.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            received.extend_from_slice(&buffer[..count]);
        }
        assert!(
            String::from_utf8(received)
                .unwrap()
                .contains("\r\nauthorization: Bearer s3cret\r\n")
        );
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let collection = cli.collection();
    let directory = Path::new(collection.as_str().unwrap());
    fs::write(
        directory.join("environment.toml"),
        format!("base = {url:?}\ntoken = \"s3cret\"\n"),
    )
    .unwrap();
    fs::write(
        directory.join(".request-eagle-collection.toml"),
        "[auth]\ntype = \"bearer\"\ntoken = \"{{token}}\"\n",
    )
    .unwrap();

    let details = cli.call(json!({"command":"collections.get","path":collection}));
    assert_eq!(
        details["auth"],
        json!({"type": "bearer", "token": "{{token}}"})
    );

    // Requests inherit the collection's authorization unless they set one.
    let created = cli.create(&collection, json!({"method":"GET", "url":"{{base}}/me"}));
    assert!(created["request"].get("auth").is_none(), "{created}");
    let response =
        cli.call(json!({"command":"requests.run","path":created["path"],"timeout_ms":5000}));
    server.join().unwrap();
    assert_eq!(response["status"], 204);

    let auth = json!({"type": "basic", "username": "me", "password": "{{password}}"});
    let updated = cli.call(json!({
        "command":"requests.update", "path":created["path"], "expected_id":created["id"],
        "request":{"method":"GET", "url":"{{base}}/me", "auth":auth}
    }));
    assert_eq!(updated["request"]["auth"], auth);
    let read = cli.call(json!({"command":"requests.get","path":created["path"]}));
    assert_eq!(read["request"]["auth"], auth);
}
