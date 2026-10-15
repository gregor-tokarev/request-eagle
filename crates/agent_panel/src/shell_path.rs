//! The `PATH` agents run with. Apps opened from the Dock or a desktop
//! launcher do not inherit the one a terminal has, which leaves out where
//! Node, Claude Code and the like are usually installed.

use std::{
    ffi::OsString,
    io::Read as _,
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// How long the login shell may take to start, as with a slow profile.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const MARKER: &str = "__REQUEST_EAGLE_PATH__";

/// The login shell's `PATH`, then the directories of the app's own that it
/// leaves out. Read once, since starting a shell takes a while; call it off
/// the UI thread.
pub(crate) fn agent_path() -> OsString {
    static PATH: OnceLock<OsString> = OnceLock::new();

    PATH.get_or_init(|| {
        let current = std::env::var_os("PATH").unwrap_or_default();
        match login_shell_path() {
            Some(login) => merge(&login, &current.to_string_lossy()).into(),
            None => current,
        }
    })
    .clone()
}

#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    // Interactive, so that profiles which set PATH only for terminals count.
    // Without a terminal on standard input, prompts they show end at once.
    let mut child = Command::new(shell)
        .args([
            "-l",
            "-i",
            "-c",
            &format!("printf '%s%s%s' {MARKER} \"$PATH\" {MARKER}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < SHELL_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;

    marked(&output).map(ToOwned::to_owned)
}

#[cfg(not(unix))]
fn login_shell_path() -> Option<String> {
    None
}

/// What the shell printed between the markers, past anything its profile
/// printed first.
pub(crate) fn marked(output: &str) -> Option<&str> {
    let (_, rest) = output.split_once(MARKER)?;
    let (path, _) = rest.split_once(MARKER)?;

    (!path.trim().is_empty()).then_some(path)
}

/// `login`'s directories, then those of `current` that it leaves out.
pub(crate) fn merge(login: &str, current: &str) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut directories: Vec<&str> = Vec::new();
    for directory in login.split(separator).chain(current.split(separator)) {
        if !directory.is_empty() && !directories.contains(&directory) {
            directories.push(directory);
        }
    }

    directories.join(&separator.to_string())
}
