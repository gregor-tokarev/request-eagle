//! The coding agents the panel runs, and where this computer has them.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// A coding agent the panel runs through the Agent Client Protocol. Agents
/// keep their own sign-in, models and tools; the panel only talks to them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Agent {
    /// How the preferences name the agent.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// The program people install, which shows the agent is there.
    pub(crate) program: &'static str,
    /// The command that starts the agent's ACP server. Claude Code and Codex
    /// speak the protocol through adapters, which run the installed agent
    /// with its sign-in. They float to their latest release, like the ACP
    /// SDK's own launchers.
    pub(crate) command: &'static [&'static str],
    /// What to do when the agent asks to sign in.
    pub(crate) sign_in: &'static str,
    pub(crate) install_url: &'static str,
}

pub(crate) const AGENTS: [Agent; 3] = [
    Agent {
        id: "claude",
        name: "Claude Code",
        program: "claude",
        command: &["npx", "-y", "@agentclientprotocol/claude-agent-acp@latest"],
        sign_in: "Run `claude` in a terminal and sign in, then start a new thread.",
        install_url: "https://docs.anthropic.com/en/docs/claude-code/setup",
    },
    Agent {
        id: "codex",
        name: "Codex",
        program: "codex",
        command: &["npx", "-y", "@agentclientprotocol/codex-acp@latest"],
        sign_in: "Run `codex login` in a terminal, then start a new thread.",
        install_url: "https://developers.openai.com/codex/cli",
    },
    Agent {
        id: "opencode",
        name: "OpenCode",
        program: "opencode",
        command: &["opencode", "acp"],
        sign_in: "Run `opencode auth login` in a terminal, then start a new thread.",
        install_url: "https://opencode.ai/docs/",
    },
];

impl Agent {
    pub(crate) fn find(id: &str) -> Option<&'static Agent> {
        AGENTS.iter().find(|agent| agent.id == id)
    }

    /// The program that starts the agent's server, when the agent and the
    /// launcher it needs, such as `npx`, are both on `path`.
    pub(crate) fn launcher(&self, path: &OsStr) -> Option<PathBuf> {
        find_program(self.program, path)?;

        find_program(self.command[0], path)
    }
}

/// Where `program` is on a `PATH`.
pub(crate) fn find_program(program: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|directory| {
        executable_names(program)
            .into_iter()
            .map(|name| directory.join(name))
            .find(|candidate| is_executable(candidate))
    })
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
