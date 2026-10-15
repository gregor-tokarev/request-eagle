use std::{ffi::OsString, fs};

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
    let claude = Agent::find("claude").unwrap();
    let opencode = Agent::find("opencode").unwrap();

    let (directory, path) = path_with(&["claude", "npx", "opencode"]);
    assert_eq!(claude.launcher(&path), Some(directory.path().join("npx")));
    assert_eq!(
        opencode.launcher(&path),
        Some(directory.path().join("opencode"))
    );

    // Claude Code speaks the protocol through an adapter that npx runs.
    let (_directory, path) = path_with(&["claude"]);
    assert_eq!(claude.launcher(&path), None);
}

#[cfg(unix)]
#[test]
fn files_that_cannot_run_do_not_count() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("opencode"), "").unwrap();
    let path = std::env::join_paths([directory.path()]).unwrap();

    assert_eq!(Agent::find("opencode").unwrap().launcher(&path), None);
}

#[test]
fn every_agent_has_its_own_id() {
    for agent in &AGENTS {
        assert_eq!(Agent::find(agent.id), Some(agent));
    }
    assert_eq!(Agent::find("cursor"), None);
}
