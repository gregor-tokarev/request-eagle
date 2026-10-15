use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use thiserror::Error;

/// How long the output may take to close after Git is done.
const OUTPUT_GRACE: Duration = Duration::from_secs(1);
/// How many characters of what Git said a failure shows.
const SUMMARY_LENGTH: usize = 100;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GitError {
    #[error("Git isn't installed.")]
    NotInstalled,
    /// What Git said when it failed. It reads as a short summary.
    #[error("{}", summary(.0))]
    Failed(String),
    #[error("Git took too long and was stopped.")]
    TimedOut,
    #[error("Add a remote first.")]
    NoRemote,
    #[error("Push this branch first.")]
    NoUpstream,
    #[error("Your changes conflict with the remote's. Resolve them with Git, then sync again.")]
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

    let stdout = Output::read(child.stdout.take());
    let stderr = Output::read(child.stderr.take());
    // Git can stop before reading it all, and then says why itself.
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(input);
    }

    let status = wait(&mut child, timeout)?;
    // A program Git started, such as a hook's, can keep the pipes open after
    // Git is done, and what Git printed is read by then.
    let until = Instant::now() + OUTPUT_GRACE;
    let stdout = stdout.collect(until);
    let stderr = stderr.collect(until);

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

/// What Git prints to a pipe, read on another thread so that Git never
/// blocks on a full pipe while it is waited for.
struct Output {
    read: Arc<Mutex<Vec<u8>>>,
    closed: mpsc::Receiver<()>,
}

impl Output {
    fn read(pipe: Option<impl Read + Send + 'static>) -> Self {
        let read = Arc::new(Mutex::new(Vec::new()));
        let (close, closed) = mpsc::channel();
        let reader = read.clone();

        thread::spawn(move || {
            let mut buffer = [0; 8192];
            if let Some(mut pipe) = pipe {
                while let Ok(count @ 1..) = pipe.read(&mut buffer) {
                    reader.lock().unwrap().extend_from_slice(&buffer[..count]);
                }
            }

            let _ = close.send(());
        });

        Self { read, closed }
    }

    /// What was printed, once the pipe closes or `until` passes.
    fn collect(self, until: Instant) -> String {
        let _ = self
            .closed
            .recv_timeout(until.saturating_duration_since(Instant::now()));

        String::from_utf8_lossy(&self.read.lock().unwrap()).into_owned()
    }
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

/// What Git said, in a sentence: what the common failures mean, or the
/// first line of others.
pub(crate) fn summary(message: &str) -> String {
    // The first that matches is taken: a remote refusing a key also fails
    // to be read from, and a lock file can fail on permissions too.
    const MEANINGS: [(&[&str], &str); 6] = [
        (
            &[
                "authentication failed",
                // SSH names the methods it tried: `(publickey,password)`.
                "permission denied (",
                "could not read username",
                "could not read password",
                "terminal prompts disabled",
                "returned error: 401",
                "returned error: 403",
            ],
            "The remote refused access. Sign in to your Git host first.",
        ),
        (
            &[
                "permission denied",
                "operation not permitted",
                "read-only file system",
            ],
            "Git can't change the repository's files. Check their permissions.",
        ),
        (
            &[".lock': file exists", "another git process"],
            "Another Git command is running. Try again shortly.",
        ),
        (
            &[
                "could not resolve host",
                "could not read from remote repository",
                "does not appear to be a git repository",
                "repository not found",
                "unable to access",
                "connection refused",
                "connection timed out",
                "network is unreachable",
            ],
            "Can't reach the remote.",
        ),
        (
            &["[rejected]", "updates were rejected", "non-fast-forward"],
            "The remote has new commits. Pull first.",
        ),
        (
            &["please tell me who you are", "unable to auto-detect email"],
            "Set your name and email in Git first.",
        ),
    ];

    let lowercase = message.to_lowercase();
    if let Some((_, meaning)) = MEANINGS
        .iter()
        .find(|(signs, _)| signs.iter().any(|sign| lowercase.contains(sign)))
    {
        return (*meaning).to_owned();
    }

    let line = message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let line = ["fatal: ", "error: ", "remote: "]
        .iter()
        .fold(line, |line, prefix| {
            line.strip_prefix(prefix).unwrap_or(line)
        });

    let mut characters = line.chars();
    let mut summary: String = match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => return "Git failed without saying why.".to_owned(),
    };
    if summary.chars().count() > SUMMARY_LENGTH {
        summary = summary.chars().take(SUMMARY_LENGTH - 1).collect();
        summary.push('…');
    }

    summary
}
