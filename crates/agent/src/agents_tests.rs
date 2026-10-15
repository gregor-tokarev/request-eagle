use std::{collections::BTreeMap, ffi::OsString, fs, path::PathBuf};

use preferences::ProviderPreferences;

use crate::agents::{AGENTS, Agent};

/// A `PATH` of one directory that holds `programs`.
fn path_with(programs: &[&str]) -> (tempfile::TempDir, OsString) {
    let directory = tempfile::tempdir().unwrap();
    for program in programs {
        let file = directory.path().join(program);
        fs::write(&file, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let path = std::env::join_paths([directory.path()]).unwrap();

    (directory, path)
}

#[cfg(unix)]
#[test]
fn an_agent_runs_when_it_and_its_launcher_are_installed() {
    let defaults = ProviderPreferences::default();
    let claude = Agent::find("claude").unwrap();
    let opencode = Agent::find("opencode").unwrap();

    let (directory, path) = path_with(&["claude", "npx", "opencode"]);
    assert_eq!(
        claude.launcher(&defaults, &path),
        Some(directory.path().join("npx"))
    );
    assert_eq!(
        opencode.launcher(&defaults, &path),
        Some(directory.path().join("opencode"))
    );

    // Claude Code speaks the protocol through an adapter that npx runs.
    let (_directory, path) = path_with(&["claude"]);
    assert_eq!(claude.launcher(&defaults, &path), None);
}

#[cfg(unix)]
#[test]
fn a_command_of_ones_own_needs_only_its_program() {
    let claude = Agent::find("claude").unwrap();
    let preferences = ProviderPreferences {
        command: "claude-agent-acp --verbose".into(),
        ..ProviderPreferences::default()
    };

    let (directory, path) = path_with(&["claude-agent-acp"]);
    assert_eq!(
        claude.launcher(&preferences, &path),
        Some(directory.path().join("claude-agent-acp"))
    );
    assert_eq!(
        claude.command(&preferences),
        ["claude-agent-acp", "--verbose"]
    );
}

#[cfg(unix)]
#[test]
fn files_that_cannot_run_do_not_count() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("opencode"), "").unwrap();
    let path = std::env::join_paths([directory.path()]).unwrap();

    assert_eq!(
        Agent::find("opencode")
            .unwrap()
            .launcher(&ProviderPreferences::default(), &path),
        None
    );
}

#[cfg(unix)]
#[test]
fn the_preferences_set_the_command_and_environment() {
    let codex = Agent::find("codex").unwrap();
    let preferences = ProviderPreferences {
        command: "'codex acp' --profile work".into(),
        config_directory: "/work/.codex".into(),
        ..ProviderPreferences::default()
    };
    let (directory, path) = path_with(&["codex acp"]);

    let launch = codex
        .launch(
            &preferences,
            path,
            BTreeMap::from([("OPENAI_BASE_URL".to_owned(), "https://proxy".to_owned())]),
            PathBuf::from("/work"),
            Vec::new(),
        )
        .unwrap();

    assert_eq!(launch.program, directory.path().join("codex acp"));
    assert_eq!(launch.args, ["--profile", "work"]);
    assert_eq!(
        launch.environment,
        BTreeMap::from([
            ("CODEX_HOME".to_owned(), "/work/.codex".to_owned()),
            ("OPENAI_BASE_URL".to_owned(), "https://proxy".to_owned()),
        ])
    );
}

#[test]
fn every_agent_has_its_own_id() {
    for agent in &AGENTS {
        assert_eq!(Agent::find(agent.id), Some(agent));
    }
    assert_eq!(Agent::find("cursor"), None);
}
