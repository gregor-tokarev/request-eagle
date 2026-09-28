mod collections;
mod commands;
mod execution;
mod settings;

use commands::{Command, FORMAT_VERSION, MAX_INPUT_BYTES};
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

const HELP: &str = "Request Eagle CLI — saved collections, requests and settings\n\nrequest-eagle-cli schema\nrequest-eagle-cli [--data-dir PATH] [--collections-dir PATH] call JSON\nrequest-eagle-cli call - < command.json\n\nUses ~/.request-eagle by default. REQUEST_EAGLE_COLLECTIONS_DIR can override\nthe collections directory. The desktop app does not need to be running.\nUse schema to discover commands and exact inputs. Run requests with requests.run;\nit waits for completion and returns the HTTP response, body and script results.\nOutput is JSON except help/version. Exit 0: success; 1: operation failed;\n2: invalid input. HTTP error statuses remain successful executions.\nQuit the desktop app before editing its files or settings; reopen it to reload.\n";

fn main() {
    let args = std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "CLI arguments must be valid UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>();

    let (code, output) = match args.and_then(parse) {
        Ok(Action::Help) => (0, HELP.to_owned()),
        Ok(Action::Version) => (
            0,
            format!(
                "request-eagle-cli {} (format {FORMAT_VERSION})\n",
                option_env!("REQUEST_EAGLE_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
            ),
        ),
        Ok(Action::Schema) => (0, success(commands::schema()).to_string()),
        Ok(Action::Call {
            data,
            collections,
            command,
        }) => {
            let preferences = preferences::PreferencesFile::new(&data);
            match smol::block_on(async {
                match *command {
                    Command::RequestsRun {
                        path,
                        trust_scripts,
                        variables,
                        timeout_ms,
                    } => {
                        execution::run(
                            &collections,
                            &preferences,
                            &path,
                            trust_scripts,
                            variables,
                            timeout_ms,
                        )
                        .await
                    }
                    command @ (Command::SettingsGet {}
                    | Command::SettingsRequest { .. }
                    | Command::SettingsAppearance { .. }
                    | Command::SettingsProxy { .. }) => {
                        settings::dispatch(&preferences, command).await
                    }
                    command => collections::dispatch(&collections, command),
                }
            }) {
                Ok(value) => (0, success(value).to_string()),
                Err(error) => (
                    1,
                    failure("operation_failed", format!("{error:#}")).to_string(),
                ),
            }
        }
        Err(error) => (2, failure("invalid_input", error).to_string()),
    };

    // A closed pipe should not turn machine-readable output into a panic.
    let result = writeln!(io::stdout().lock(), "{}", output.trim_end());
    std::process::exit(if result.is_err() { 1 } else { code });
}

enum Action {
    Help,
    Version,
    Schema,
    Call {
        data: PathBuf,
        collections: PathBuf,
        command: Box<Command>,
    },
}

fn parse(mut args: Vec<String>) -> Result<Action, String> {
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        return Ok(Action::Help);
    }
    if args == ["--version"] {
        return Ok(Action::Version);
    }
    if args == ["schema"] {
        return Ok(Action::Schema);
    }

    let mut data = None;
    let mut collections = None;
    while args.first().is_some_and(|arg| arg.starts_with("--")) {
        if args.len() < 2 {
            return Err("Option needs a value".into());
        }
        let flag = args.remove(0);
        let value = args.remove(0);
        match flag.as_str() {
            "--data-dir" => data = Some(PathBuf::from(value)),
            "--collections-dir" => collections = Some(PathBuf::from(value)),
            _ => return Err(format!("Unknown option {flag}")),
        }
    }

    let [command, input] = args.as_slice() else {
        return Err("Expected schema or call JSON (use --help)".into());
    };
    if command != "call" {
        return Err("Expected call JSON (use --help)".into());
    }
    let input = if input == "-" {
        let mut bytes = Vec::new();
        io::stdin()
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "Command input must be valid UTF-8".to_owned())?
    } else {
        input.clone()
    };
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err("Command exceeds input limit".into());
    }
    let command = serde_json::from_str(&input).map_err(|e| format!("Invalid command: {e}"))?;

    let data = data
        .or_else(|| dirs::home_dir().map(|home| home.join(".request-eagle")))
        .ok_or("Home directory unavailable; use --data-dir")?;
    let data = std::path::absolute(data).map_err(|e| e.to_string())?;
    let collections = collections
        .or_else(|| std::env::var_os("REQUEST_EAGLE_COLLECTIONS_DIR").map(PathBuf::from))
        .unwrap_or_else(|| data.join("collections"));
    let collections = std::path::absolute(collections).map_err(|e| e.to_string())?;
    if data.to_str().is_none() || collections.to_str().is_none() {
        return Err("Storage paths must be valid UTF-8".into());
    }

    Ok(Action::Call {
        data,
        collections,
        command,
    })
}

fn success(value: Value) -> Value {
    json!({"version": FORMAT_VERSION, "ok": true, "result": value})
}

fn failure(code: &str, message: String) -> Value {
    json!({"version": FORMAT_VERSION, "ok": false, "error": {"code": code, "message": message}})
}
