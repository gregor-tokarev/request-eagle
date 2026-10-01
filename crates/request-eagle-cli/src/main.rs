mod collections;
mod commands;
mod cookies;
mod execution;
mod settings;

use clap::{Parser, Subcommand, error::ErrorKind};
use commands::{Command, FORMAT_VERSION, MAX_INPUT_BYTES};
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    name = "request-eagle-cli",
    bin_name = "request-eagle-cli",
    version = format!(
        "{} (format {FORMAT_VERSION})",
        option_env!("REQUEST_EAGLE_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
    ),
    about = "Manage Request Eagle's saved collections, requests and settings",
    arg_required_else_help = true,
    after_help = "Use schema to discover commands and exact inputs. Run requests with requests.run;\n\
        it waits for completion and returns the HTTP response, body and script results.\n\
        Output is JSON except help/version. Exit 0: success; 1: operation failed;\n\
        2: invalid input. HTTP error statuses remain successful executions.\n\
        The desktop app does not need to be running. Quit it before editing its files\n\
        or settings; reopen it to reload."
)]
struct Cli {
    /// Data directory [default: ~/.request-eagle]
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<String>,

    /// Collections directory [default: REQUEST_EAGLE_COLLECTIONS_DIR or DATA_DIR/collections]
    #[arg(long, global = true, value_name = "PATH")]
    collections_dir: Option<String>,

    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Print the JSON command schema
    Schema,
    /// Execute a JSON command
    Call {
        /// JSON command, or - to read it from stdin (maximum 8 MiB)
        #[arg(value_name = "JSON")]
        input: String,
    },
}

fn main() {
    let (code, output) = match parse() {
        Ok(Action::Display(output)) => (0, output),
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
                            &data.join("cookies.json"),
                            &path,
                            trust_scripts,
                            variables,
                            timeout_ms,
                        )
                        .await
                    }
                    command @ (Command::CookiesList { .. } | Command::CookiesDelete { .. }) => {
                        cookies::dispatch(&data.join("cookies.json"), command)
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
    Display(String),
    Schema,
    Call {
        data: PathBuf,
        collections: PathBuf,
        command: Box<Command>,
    },
}

fn parse() -> Result<Action, String> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion => return Ok(Action::Display(error.to_string())),
            _ => return Err(error.to_string()),
        },
    };

    let input = match cli.command {
        CliCommand::Schema => return Ok(Action::Schema),
        CliCommand::Call { input } => input,
    };

    let input = if input == "-" {
        let mut bytes = Vec::new();
        io::stdin()
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "Command input must be valid UTF-8".to_owned())?
    } else {
        input
    };
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err("Command exceeds input limit".into());
    }
    let command = serde_json::from_str(&input).map_err(|e| format!("Invalid command: {e}"))?;

    let data = cli
        .data_dir
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".request-eagle")))
        .ok_or("Home directory unavailable; use --data-dir")?;
    let data = std::path::absolute(data).map_err(|e| e.to_string())?;
    let collections = cli
        .collections_dir
        .map(PathBuf::from)
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
