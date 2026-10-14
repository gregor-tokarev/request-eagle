use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GitError {
    #[error("Git isn't installed, or Request Eagle can't run it.")]
    NotInstalled,
    /// What Git said when it failed.
    #[error("{0}")]
    Failed(String),
    #[error("Git took too long to answer and was stopped.")]
    TimedOut,
    #[error("This repository has no remote to push to. Add one first.")]
    NoRemote,
    #[error("This branch doesn't track a remote branch yet. Push it first.")]
    NoUpstream,
    #[error(
        "Your changes conflict with the remote's. Resolve the conflicts in your editor or \
         terminal and finish with Git."
    )]
    Conflicts,
    #[error("{0}")]
    Io(String),
}

impl From<io::Error> for GitError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::TimedOut => Self::TimedOut,
            _ => Self::Io(error.to_string()),
        }
    }
}

/// Runs Git in `directory` and returns what it printed. Git never asks in
/// a terminal, which nobody may be watching, so a missing password fails
/// instead of waiting. With `ask`, a program Git starts can still ask in a
/// window of its own, as SSH does for a key's passphrase when the system
/// offers one. `input` goes to Git's standard input. Past `timeout`, Git is
/// stopped, as when a hook hangs.
pub(crate) fn run(
    directory: &Path,
    args: &[&OsStr],
    input: Option<&[u8]>,
    timeout: Duration,
    ask: bool,
) -> Result<String, GitError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        // Collection and request names can hold `*` and `[`, which Git would
        // read as patterns.
        .args(["--literal-pathspecs", "-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        // Reading the status must not hold up the user's own Git commands.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if !ask {
        command.env("SSH_ASKPASS_REQUIRE", "never");
    }

    // Without a terminal of its own, SSH cannot ask in the one Request Eagle
    // was started from.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // SAFETY: `setsid` is async-signal-safe and touches no memory of the
        // parent process.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    // A console window would flash up for every command.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => GitError::NotInstalled,
        _ => error.into(),
    })?;

    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    // Git can stop before reading it all, and then says why itself.
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(input);
    }

    // Past the timeout the readers are left behind: a program Git started,
    // such as SSH, can keep the pipes open.
    let status = wait(&mut child, timeout)?;
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();

    if status.success() {
        Ok(stdout)
    } else {
        Err(GitError::Failed(message(&stderr, &stdout)))
    }
}

fn wait(child: &mut Child, timeout: Duration) -> io::Result<std::process::ExitStatus> {
    let started = Instant::now();
    // Most commands finish within milliseconds, so the pauses start short.
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }

        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
        }

        thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(50));
    }
}

/// Reads a pipe to its end on another thread, so that Git never blocks on a
/// full pipe while it is waited for.
fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut output);
        }

        String::from_utf8_lossy(&output).into_owned()
    })
}

/// What Git said went wrong, without its hints on which commands to run.
fn message(stderr: &str, stdout: &str) -> String {
    let output = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };

    let message = output
        .lines()
        .filter(|line| !line.starts_with("hint:"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();

    if message.is_empty() {
        "Git failed without saying why.".to_owned()
    } else {
        message
    }
}
