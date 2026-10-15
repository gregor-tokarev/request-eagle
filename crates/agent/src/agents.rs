//! The coding agents the panel runs, where this computer has them, and how
//! their preferences start them.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use preferences::ProviderPreferences;

use crate::connection::Launch;

/// A coding agent the panel runs through the Agent Client Protocol. Agents
/// keep their own sign-in, models and tools; the panel only talks to them.
#[derive(Debug, PartialEq, Eq)]
pub struct Agent {
    /// How the preferences name the agent.
    pub id: &'static str,
    pub name: &'static str,
    /// The program people install, which shows the agent is there.
    pub program: &'static str,
    /// The command that starts the agent's ACP server. Claude Code and Codex
    /// speak the protocol through adapters, which run the installed agent
    /// with its sign-in. They float to their latest release, like the ACP
    /// SDK's own launchers.
    pub command: &'static [&'static str],
    /// The environment variable that moves the agent's settings and sign-in
    /// to another folder.
    pub config_variable: &'static str,
    /// Where the agent keeps its settings unless told otherwise.
    pub default_config_directory: &'static str,
    /// The name of the agent's mode that runs tools without asking, which
    /// the panel starts it in unless told to ask.
    pub bypass_mode: Option<&'static str>,
    /// What to do when the agent asks to sign in.
    pub sign_in: &'static str,
    pub install_url: &'static str,
    pub icon: &'static str,
}

pub const AGENTS: [Agent; 3] = [
    Agent {
        id: "claude",
        name: "Claude Code",
        program: "claude",
        command: &["npx", "-y", "@agentclientprotocol/claude-agent-acp@latest"],
        config_variable: "CLAUDE_CONFIG_DIR",
        default_config_directory: "~/.claude",
        bypass_mode: Some("Bypass permissions"),
        sign_in: "Run `claude` in a terminal and sign in, then start a new thread.",
        install_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
        icon: "icons/claude.svg",
    },
    Agent {
        id: "codex",
        name: "Codex",
        program: "codex",
        command: &["npx", "-y", "@agentclientprotocol/codex-acp@latest"],
        config_variable: "CODEX_HOME",
        default_config_directory: "~/.codex",
        bypass_mode: Some("Full access"),
        sign_in: "Run `codex login` in a terminal, then start a new thread.",
        install_url: "https://developers.openai.com/codex/cli",
        icon: "icons/square-terminal.svg",
    },
    Agent {
        id: "opencode",
        name: "OpenCode",
        program: "opencode",
        command: &["opencode", "acp"],
        config_variable: "OPENCODE_CONFIG_DIR",
        default_config_directory: "~/.config/opencode",
        bypass_mode: None,
        sign_in: "Run `opencode auth login` in a terminal, then start a new thread.",
        install_url: "https://opencode.ai/docs/",
        icon: "icons/opencode.svg",
    },
];

impl Agent {
    pub fn find(id: &str) -> Option<&'static Agent> {
        AGENTS.iter().find(|agent| agent.id == id)
    }

    /// The default command, as the preferences show it.
    pub fn default_command(&self) -> String {
        self.command.join(" ")
    }

    /// The command that starts the agent's ACP server: the one in its
    /// preferences, or the default.
    pub fn command(&self, preferences: &ProviderPreferences) -> Vec<String> {
        let custom = preferences.command.trim();
        if custom.is_empty() {
            return self.command.iter().map(|part| (*part).to_owned()).collect();
        }

        shell_words::split(custom)
            .unwrap_or_else(|_| custom.split_whitespace().map(ToOwned::to_owned).collect())
    }

    /// The program that starts the agent's server, when it is on `path` and
    /// so is the agent. A command of the user's own needs only its program.
    pub fn launcher(&self, preferences: &ProviderPreferences, path: &OsStr) -> Option<PathBuf> {
        let command = self.command(preferences);
        let program = command.first()?;
        if preferences.command.trim().is_empty() {
            find_program(self.program, path)?;
        }

        let program = expand_home(program);
        if program.components().count() > 1 {
            return is_executable(&program).then_some(program);
        }
        find_program(&program.to_string_lossy(), path)
    }

    /// How to start the agent working in `directory`, unless it is not
    /// installed. `environment` holds the variables from its preferences.
    pub fn launch(
        &self,
        preferences: &ProviderPreferences,
        path: OsString,
        mut environment: BTreeMap<String, String>,
        directory: PathBuf,
        additional_directories: Vec<PathBuf>,
    ) -> Option<Launch> {
        let program = self.launcher(preferences, &path)?;
        let args = self.command(preferences).split_off(1);
        let config_directory = preferences.config_directory.trim();
        if !config_directory.is_empty() {
            environment.insert(
                self.config_variable.to_owned(),
                expand_home(config_directory).to_string_lossy().into_owned(),
            );
        }

        Some(Launch {
            program,
            args,
            path,
            environment,
            directory,
            additional_directories,
        })
    }
}

/// Where `program` is on a `PATH`.
pub fn find_program(program: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|directory| {
        executable_names(program)
            .into_iter()
            .map(|name| directory.join(name))
            .find(|candidate| is_executable(candidate))
    })
}

/// A path that starts with `~`, from the home folder.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => std::env::home_dir().unwrap_or_else(|| path.into()),
        _ => PathBuf::from(path),
    }
}

/// Windows finds `npx.cmd` and `claude.exe` for `npx` and `claude`.
fn executable_names(program: &str) -> Vec<OsString> {
    if !cfg!(windows) {
        return vec![program.into()];
    }

    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
    std::iter::once(OsString::from(program))
        .chain(
            extensions
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| OsString::from(format!("{program}{extension}"))),
        )
        .collect()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}
