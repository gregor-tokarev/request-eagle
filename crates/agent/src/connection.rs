//! The agent's process, and the conversation with it over the Agent Client
//! Protocol. The panel sends commands and reads events; the protocol runs
//! in a task the connection owns, and ending the connection ends the agent.

use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, ErrorCode, InitializeRequest, NewSessionRequest,
    PromptRequest, RequestPermissionRequest, RequestPermissionResponse, SessionConfigId,
    SessionConfigOption, SessionConfigValueId, SessionId, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, StopReason,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, LineDirection, Responder,
};
use futures::{
    FutureExt as _, StreamExt as _,
    channel::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
        oneshot,
    },
    future::{Either, Shared, select},
};
use gpui_kit::{App, Task};

/// How many lines of what the agent wrote to stderr a failure shows.
const STDERR_LINES: usize = 12;

/// How to start an agent, and where it works.
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The `PATH` the agent runs with, since the tools it starts need it.
    pub path: OsString,
    /// Variables from the agent's preferences, such as an API key.
    pub environment: BTreeMap<String, String>,
    pub directory: PathBuf,
    /// Other folders the agent may work in, such as the app's data.
    pub additional_directories: Vec<PathBuf>,
}

pub enum Command {
    Prompt(Vec<ContentBlock>),
    Cancel,
    SetOption {
        id: SessionConfigId,
        value: SessionConfigValueId,
    },
}

pub enum Event {
    /// The session started, and the agent takes prompts.
    Started {
        /// Whether prompts can carry cites as embedded resources.
        embedded_context: bool,
        options: Vec<SessionConfigOption>,
    },
    Update(SessionUpdate),
    /// The agent asks before running a tool. It waits for the answer.
    Permission {
        request: RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    },
    /// The agent finished answering a prompt.
    TurnEnded(Result<StopReason, String>),
    Options(Vec<SessionConfigOption>),
    /// The agent turned down a change to its options, and says why.
    OptionRejected(String),
    /// The agent could not start, or it stopped.
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    /// Whether the agent asks to sign in first.
    pub sign_in: bool,
}

pub struct Connection {
    commands: UnboundedSender<Command>,
    _task: Task<()>,
}

impl Connection {
    /// Start the agent and a session in `launch.directory`. Events arrive on
    /// the receiver until the agent stops or the connection is dropped.
    pub fn start(launch: Launch, cx: &mut App) -> (Self, UnboundedReceiver<Event>) {
        let (commands, command_stream) = unbounded();
        let (events, event_stream) = unbounded();

        let task = cx.spawn(async move |_| {
            let failed = events.clone();
            if let Err(failure) = run(launch, command_stream, events).await {
                let _ = failed.unbounded_send(Event::Failed(failure));
            }
        });

        (
            Self {
                commands,
                _task: task,
            },
            event_stream,
        )
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.unbounded_send(command);
    }
}

async fn run(
    launch: Launch,
    mut commands: UnboundedReceiver<Command>,
    events: UnboundedSender<Event>,
) -> Result<(), Failure> {
    let stderr = Arc::new(Mutex::new(VecDeque::new()));
    let agent = AcpAgent::new(
        AcpAgentConfig::new(launch.program)
            .args(launch.args)
            .env("PATH", launch.path.to_string_lossy())
            .envs(launch.environment),
    )
    .with_debug({
        let stderr = stderr.clone();
        move |line, direction| {
            if direction == LineDirection::Stderr {
                let mut lines = stderr.lock().unwrap();
                if lines.len() == STDERR_LINES {
                    lines.pop_front();
                }
                lines.push_back(line.to_owned());
            }
        }
    });

    let updates = events.clone();
    let permissions = events.clone();
    let directory = launch.directory;
    let additional_directories = launch.additional_directories;

    let result = Client
        .builder()
        .name("request-eagle")
        .on_receive_notification(
            async move |notification: SessionNotification, _| {
                let _ = updates.unbounded_send(Event::Update(notification.update));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // Answered later, from the panel, so the agent's other messages
        // keep arriving meanwhile.
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _| {
                let _ = permissions.unbounded_send(Event::Permission { request, responder });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let initialized = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1).client_info(
                        agent_client_protocol::schema::v1::Implementation::new(
                            "request-eagle",
                            env!("CARGO_PKG_VERSION"),
                        )
                        .title("Request Eagle".to_owned()),
                    ),
                )
                .block_task()
                .await?;
            let session = connection
                .send_request(
                    NewSessionRequest::new(directory)
                        .additional_directories(additional_directories),
                )
                .block_task()
                .await?;
            let id = session.session_id;

            let _ = events.unbounded_send(Event::Started {
                embedded_context: initialized
                    .agent_capabilities
                    .prompt_capabilities
                    .embedded_context,
                options: session.config_options.unwrap_or_default(),
            });

            // The option changes not answered yet, which a prompt waits for,
            // and how many times the turn was stopped.
            let mut pending_options: Option<PendingOptions> = None;
            let stops = Arc::new(AtomicU64::new(0));
            loop {
                let next = select(commands.next(), Box::pin(connection.incoming_closed())).await;
                let command = match next {
                    Either::Left((Some(command), _)) => command,
                    // The panel ended the session.
                    Either::Left((None, _)) => return Ok(()),
                    Either::Right(_) => {
                        return Err(agent_client_protocol::Error::internal_error()
                            .data(serde_json::json!("The agent stopped.")));
                    }
                };

                handle(
                    command,
                    &id,
                    &connection,
                    &events,
                    &mut pending_options,
                    &stops,
                )?;
            }
        })
        .await;

    result.map_err(|error| {
        let lines: Vec<String> = stderr.lock().unwrap().iter().cloned().collect();
        failure(error, &lines)
    })
}

/// Completes once the option changes sent so far are answered.
type PendingOptions = Shared<oneshot::Receiver<()>>;

fn handle(
    command: Command,
    id: &SessionId,
    connection: &ConnectionTo<Agent>,
    events: &UnboundedSender<Event>,
    pending_options: &mut Option<PendingOptions>,
    stops: &Arc<AtomicU64>,
) -> Result<(), agent_client_protocol::Error> {
    match command {
        // A prompt sent after a change of mode waits for it, so that it runs
        // in that mode. Stopping meanwhile drops it.
        Command::Prompt(blocks) => {
            let pending = pending_options.clone();
            let connection = connection.clone();
            let id = id.clone();
            let events = events.clone();
            let stops = stops.clone();
            let stopped_before = stops.load(Ordering::SeqCst);
            connection.clone().spawn(async move {
                if let Some(pending) = pending {
                    let _ = pending.await;
                    if stops.load(Ordering::SeqCst) != stopped_before {
                        let _ = events.unbounded_send(Event::TurnEnded(Ok(StopReason::Cancelled)));
                        return Ok(());
                    }
                }
                let result = connection
                    .send_request(PromptRequest::new(id, blocks))
                    .block_task()
                    .await
                    .map(|response| response.stop_reason)
                    .map_err(|error| error.message);
                let _ = events.unbounded_send(Event::TurnEnded(result));
                Ok(())
            })
        }
        // Goes out at once, also while an option change waits.
        Command::Cancel => {
            stops.fetch_add(1, Ordering::SeqCst);
            connection.send_notification(CancelNotification::new(id.clone()))
        }
        Command::SetOption { id: option, value } => {
            let request = connection.send_request(SetSessionConfigOptionRequest::new(
                id.clone(),
                option,
                value,
            ));
            let previous = pending_options.take();
            let (answered, pending) = oneshot::channel();
            *pending_options = Some(pending.shared());
            let events = events.clone();
            connection.spawn(async move {
                let event = match request.block_task().await {
                    Ok(response) => Event::Options(response.config_options),
                    Err(error) => Event::OptionRejected(error.message),
                };
                let _ = events.unbounded_send(event);
                if let Some(previous) = previous {
                    let _ = previous.await;
                }
                let _ = answered.send(());
                Ok(())
            })
        }
    }
}

/// Why the agent could not start or stopped, from its error and the last
/// lines it wrote to stderr.
pub(crate) fn failure(error: agent_client_protocol::Error, stderr: &[String]) -> Failure {
    if error.code == ErrorCode::AuthRequired {
        return Failure {
            message: error.message,
            sign_in: true,
        };
    }

    let detail = error
        .data
        .as_ref()
        .and_then(|data| data.as_str().map(ToOwned::to_owned))
        .unwrap_or_default();
    let mut message = match (error.message.as_str(), detail.as_str()) {
        (message, "") => message.to_owned(),
        (_, detail) => detail.to_owned(),
    };
    let stderr: Vec<&str> = stderr
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if !stderr.is_empty() {
        message.push_str("\n\n");
        message.push_str(&stderr.join("\n"));
    }

    Failure {
        message,
        sign_in: false,
    }
}
