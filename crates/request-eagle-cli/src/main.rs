use request_eagle_automation::{
    Call, Command, MAX_MESSAGE_BYTES, PROTOCOL_VERSION, failure, schema, socket_directory, success,
    validate_directory, validate_socket,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

const HELP: &str = "Request Eagle CLI — control the running app with JSON\n\nrequest-eagle-cli schema\nrequest-eagle-cli instances\nrequest-eagle-cli [--socket PATH] [--timeout-ms N] call JSON\nrequest-eagle-cli [--socket PATH] call - < command.json\n\nUse schema to discover commands and their exact inputs. Output is always JSON\nexcept help/version. Exit 0 means success; 1 means an operation/connection failed;\n2 means invalid CLI input. No interactive prompts. Multiple app instances require\n--socket. Enable CLI access in General settings and copy the session command\n(REQUEST_EAGLE_CLI_TOKEN) before connecting. Request execution is asynchronous: send, then poll responses.get.\n";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args = std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| (2, "CLI arguments must be valid UTF-8".to_owned()))
        })
        .collect::<Result<Vec<_>, _>>();

    match args.and_then(execute) {
        Ok(value) => {
            let ok = value["ok"] == true;
            println!("{value}");
            if ok { 0 } else { 1 }
        }
        Err((code, message)) => {
            println!(
                "{}",
                failure(
                    if code == 2 {
                        "invalid_input"
                    } else {
                        "connection_error"
                    },
                    message
                )
            );
            code
        }
    }
}

fn execute(mut args: Vec<String>) -> Result<Value, (i32, String)> {
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        print!("{HELP}");
        std::process::exit(0);
    }
    if args == ["--version"] {
        println!(
            "request-eagle-cli {} (protocol {})",
            option_env!("REQUEST_EAGLE_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
            PROTOCOL_VERSION
        );
        std::process::exit(0);
    }

    let mut socket = None;
    let mut timeout = Duration::from_secs(30);
    while args.first().is_some_and(|arg| arg.starts_with("--")) {
        if args.len() < 2 {
            return Err((2, "Option needs a value".into()));
        }
        let flag = args.remove(0);
        let value = args.remove(0);
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value)),
            "--timeout-ms" => {
                let millis: u64 = value.parse().map_err(|_| (2, "Invalid timeout".into()))?;
                if millis == 0 {
                    return Err((2, "Timeout must be positive".into()));
                }
                timeout = Duration::from_millis(millis);
            }
            _ => return Err((2, format!("Unknown option {flag}"))),
        }
    }

    match args.as_slice() {
        [command] if command == "schema" => Ok(success(schema())),
        [command] if command == "instances" => Ok(success(json!(instances()?))),
        [command, input] if command == "call" => {
            let input = if input == "-" {
                let mut bytes = Vec::new();
                io::stdin()
                    .take(MAX_MESSAGE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| (2, e.to_string()))?;
                String::from_utf8(bytes).map_err(|e| (2, e.to_string()))?
            } else {
                input.clone()
            };
            if input.len() as u64 > MAX_MESSAGE_BYTES {
                return Err((2, "Command exceeds message limit".into()));
            }
            let command: Command = serde_json::from_str(&input).map_err(|e| (2, e.to_string()))?;
            let path = match socket {
                Some(path) => path,
                None => {
                    let available = instances()?;
                    if available.len() != 1 {
                        return Err((
                            1,
                            format!(
                                "Expected one running app, found {}. Start Request Eagle, enable CLI access in General settings, or use instances and --socket.",
                                available.len()
                            ),
                        ));
                    }
                    PathBuf::from(available[0]["socket"].as_str().unwrap())
                }
            };
            call(&path, command, timeout).map_err(|e| (1, e.to_string()))
        }
        _ => Err((
            2,
            "Expected schema, instances, or call JSON (use --help)".into(),
        )),
    }
}

fn instances() -> Result<Vec<Value>, (i32, String)> {
    let directory = socket_directory().map_err(|e| (1, e.to_string()))?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    validate_directory(&directory).map_err(|e| (1, e.to_string()))?;
    let entries = fs::read_dir(directory).map_err(|e| (1, e.to_string()))?;
    let mut instances = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| (1, e.to_string()))?.path();
        if !path.extension().is_some_and(|ext| ext == "sock") {
            continue;
        }

        let socket_name = path
            .to_str()
            .ok_or_else(|| (1, "Automation socket paths must be valid UTF-8".to_owned()))?;

        match call(&path, Command::AppStatus {}, Duration::from_millis(500)) {
            Ok(reply) => instances.push(json!({
                "socket": socket_name,
                "app": reply.get("result"),
                "error": reply.get("error"),
            })),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                // Only a definitely absent listener is stale. A busy app may
                // accept the connection but miss the short discovery deadline.
            }
            Err(error) => instances.push(json!({
                "socket": socket_name,
                "app": null,
                "error": error.to_string(),
            })),
        }
    }
    instances.sort_by_key(|value| value["socket"].as_str().unwrap().to_owned());
    Ok(instances)
}

fn call(path: &Path, command: Command, timeout: Duration) -> io::Result<Value> {
    validate_socket(path)?;
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut bytes = serde_json::to_vec(&Call {
        version: PROTOCOL_VERSION,
        token: std::env::var("REQUEST_EAGLE_CLI_TOKEN").unwrap_or_default(),
        command,
    })?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    let mut reply = Vec::new();
    BufReader::new(stream)
        .take(MAX_MESSAGE_BYTES + 1)
        .read_until(b'\n', &mut reply)?;
    if reply.len() as u64 > MAX_MESSAGE_BYTES || reply.last() != Some(&b'\n') {
        return Err(io::Error::other("Incomplete or oversized reply"));
    }
    let value: Value = serde_json::from_slice(&reply)?;
    if value["version"] != PROTOCOL_VERSION || !value["ok"].is_boolean() {
        return Err(io::Error::other(
            "Incompatible automation protocol; install the CLI for this app version",
        ));
    }
    Ok(value)
}
