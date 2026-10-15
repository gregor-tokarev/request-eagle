//! A check of an agent, as the settings show it: its version, whether it is
//! ready to run, and the models it offers.

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};

use futures::{
    FutureExt as _, StreamExt as _,
    future::{Either, select},
};
use gpui_kit::{App, AsyncApp, Task};
use preferences::{ModelPreference, ProviderPreferences};

use crate::agents::{Agent, find_program};
use crate::connection::{Connection, Event};
use crate::{options, shell_path};

/// How long an agent may take to start for a check, such as while `npx`
/// downloads its adapter.
const START_TIMEOUT: Duration = Duration::from_secs(120);
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Check {
    /// The installed agent's version, such as `2.1.300`.
    pub version: Option<String>,
    pub status: Status,
    pub models: Vec<ModelPreference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Ready,
    SignInNeeded,
    Failed(String),
}

/// Start the agent as the panel would, read what it offers and end it.
pub fn check(
    agent: &'static Agent,
    preferences: ProviderPreferences,
    environment: BTreeMap<String, String>,
    directory: PathBuf,
    cx: &mut App,
) -> Task<Check> {
    cx.spawn(async move |cx| {
        let (launch, version) = cx
            .background_executor()
            .spawn(async move {
                let path = shell_path::agent_path();
                let version = installed_version(agent, &path);
                let launch = agent.launch(&preferences, path, environment, directory, Vec::new());

                (launch, version)
            })
            .await;
        let Some(launch) = launch else {
            return Check {
                version,
                status: Status::NotInstalled,
                models: Vec::new(),
            };
        };

        let (status, models) = start(launch, cx).await;
        Check {
            version,
            status,
            models,
        }
    })
}

async fn start(launch: crate::Launch, cx: &mut AsyncApp) -> (Status, Vec<ModelPreference>) {
    let (connection, mut events) = cx.update(|cx| Connection::start(launch, cx));
    let timeout = cx.background_executor().timer(START_TIMEOUT).fuse();
    futures::pin_mut!(timeout);

    loop {
        let next = select(events.next(), &mut timeout).await;
        let event = match next {
            Either::Left((Some(event), _)) => event,
            Either::Left((None, _)) => {
                return (Status::Failed("The agent stopped.".into()), Vec::new());
            }
            Either::Right(_) => {
                return (
                    Status::Failed("The agent took too long to start.".into()),
                    Vec::new(),
                );
            }
        };

        match event {
            Event::Started { options, .. } => {
                drop(connection);
                return (Status::Ready, options::offered_models(&options));
            }
            Event::Failed(failure) if failure.sign_in => {
                return (Status::SignInNeeded, Vec::new());
            }
            Event::Failed(failure) => return (Status::Failed(failure.message), Vec::new()),
            _ => {}
        }
    }
}

/// What the agent's program says its version is.
fn installed_version(agent: &Agent, path: &std::ffi::OsStr) -> Option<String> {
    let program = find_program(agent.program, path)?;
    let mut child = std::process::Command::new(program)
        .arg("--version")
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let started = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > VERSION_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut output = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut output).ok()?;
    parse_version(&output)
}

/// The version in what a program prints for `--version`, such as `2.1.300`
/// in `2.1.300 (Claude Code)` or `0.162.1` in `codex-cli 0.162.1`.
pub(crate) fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.starts_with(|character: char| character.is_ascii_digit()))
        .map(ToOwned::to_owned)
}
