use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    path::PathBuf,
};

use agent::acp::Responder;
use agent::acp::schema::v1::{
    ContentBlock, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigOption,
    SessionConfigOptionCategory, SessionUpdate, StopReason, ToolCallStatus, ToolKind,
};
use agent::{AGENTS, Agent, Command, Connection, Event, Failure};
use futures::StreamExt as _;
use gpui_kit::component::{
    input::{InlineToken, InputContent, RopeExt as _, TextareaState},
    message_scroller::MessageScrollerState,
};
use gpui_kit::*;
use preferences::{Preferences, ProviderPreferences};
use tab_ui::{Cite, CiteOrigin};
use tempfile::TempPath;

use crate::prompt::{Briefing, prompt};
use crate::thread::{Entry, Notice, Thread, UserMessage};

pub enum AgentPanelEvent {
    /// The agent may have changed saved requests or flows, so the app reads
    /// them again.
    FilesChanged,
    /// Show the tab a cite comes from, or its saved request.
    ShowCite(CiteOrigin),
}

/// The agent the panel talks to, and the thread with it. The agent runs as
/// a process of its own, started when the panel is first shown and ended
/// with the thread.
pub struct AgentPanel {
    pub(crate) agent: &'static Agent,
    /// The agents found on this computer and turned on in the settings, once
    /// looked for, and the `PATH` they run with.
    pub(crate) installed: Option<(Vec<&'static Agent>, OsString)>,
    looking: Option<Task<()>>,
    /// Reading the agent's environment variables before it starts.
    pub(crate) starting: Option<Task<()>>,
    pub(crate) briefing: Briefing,
    /// Where the next thread's agent works.
    pub(crate) directory: PathBuf,
    pub(crate) session: Option<Session>,
    pub(crate) failure: Option<Failure>,
    /// Whether the agent is answering a message.
    pub(crate) running: bool,
    /// A message sent while the agent was starting.
    queued: Option<(InputContent, Vec<(String, Cite)>)>,
    /// Whether the thread's first message, which briefs the agent, was sent.
    briefed: bool,
    /// Long cites saved for the agent to read, deleted with the thread.
    cite_files: Vec<TempPath>,

    pub(crate) thread: Thread,
    pub(crate) scroller: Entity<MessageScrollerState>,
    /// Sent messages, shown with their cites, by entry.
    pub(crate) messages: HashMap<usize, Entity<TextareaState>>,
    /// The thoughts and tool calls opened to show their text, by entry.
    pub(crate) expanded: HashSet<usize>,
    /// The focus of the rows that open them, by entry.
    pub(crate) disclosures: HashMap<usize, FocusHandle>,
    /// The tool calls the agent asks to run, by their ID.
    pub(crate) permissions: HashMap<String, Permission>,

    pub(crate) composer: Entity<TextareaState>,
    /// The agent the message box's placeholder names.
    pub(crate) placeholder: Option<&'static Agent>,
    /// The cites in the message being written, by the ID of their token.
    pub(crate) cites: Vec<(String, Cite)>,
    next_cite: usize,
    pub(crate) focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

pub(crate) struct Session {
    connection: Connection,
    /// Where the agent works.
    pub(crate) directory: PathBuf,
    /// Whether the settings ask the agent to ask before running tools.
    ask_before_tools: bool,
    /// Whether the panel allows the tools the agent asks about, as it does
    /// while the agent runs in its mode that does not ask.
    auto_approve: bool,
    /// Set once the agent takes prompts.
    pub(crate) ready: Option<Ready>,
    _events: Task<()>,
}

pub(crate) struct Ready {
    embedded_context: bool,
    /// The agent's options as shown, with a change made here before the
    /// agent confirms it.
    pub(crate) options: Vec<SessionConfigOption>,
    /// The options as the agent last confirmed them.
    confirmed: Vec<SessionConfigOption>,
}

pub(crate) struct Permission {
    pub(crate) options: Vec<PermissionOption>,
    responder: Responder<RequestPermissionResponse>,
}

impl EventEmitter<AgentPanelEvent> for AgentPanel {}

impl AgentPanel {
    /// `data_directory` is where the app keeps collections and flows, and
    /// where the agent works unless told otherwise.
    pub fn new(data_directory: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent = cx
            .try_global::<Preferences>()
            .and_then(|preferences| preferences.agents.selected.as_deref())
            .and_then(Agent::find)
            .unwrap_or(&AGENTS[0]);
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .submit_on_enter(true)
        });
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let subscriptions = vec![
            cx.subscribe_in(&composer, window, Self::on_composer_event),
            cx.observe(&scroller, |_, _, cx| cx.notify()),
            // Agents turned on or off, or set up differently, in the settings.
            cx.observe_global::<Preferences>(|this, cx| {
                if this.installed.is_some() {
                    this.look_for_agents(false, cx);
                }
                cx.notify();
            }),
        ];

        Self {
            agent,
            installed: None,
            looking: None,
            starting: None,
            briefing: Briefing {
                data_directory: data_directory.clone(),
                cli: None,
            },
            directory: data_directory,
            session: None,
            failure: None,
            running: false,
            queued: None,
            briefed: false,
            cite_files: Vec::new(),
            thread: Thread::default(),
            scroller,
            messages: HashMap::new(),
            expanded: HashSet::new(),
            disclosures: HashMap::new(),
            permissions: HashMap::new(),
            composer,
            placeholder: None,
            cites: Vec::new(),
            next_cite: 1,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Get the agent ready when the panel is shown. Finding the agents
    /// starts a login shell, so it waits until then.
    pub fn shown(&mut self, cx: &mut Context<Self>) {
        if self.installed.is_none() {
            self.look_for_agents(true, cx);
        } else if self.session.is_none() && self.failure.is_none() {
            self.start(cx);
        }
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    /// Where the next thread's agent works, such as the repository of the
    /// collection in the active tab.
    pub fn set_directory(&mut self, directory: PathBuf, cx: &mut Context<Self>) {
        if self.directory != directory {
            self.directory = directory;
            cx.notify();
        }
    }

    /// The settings of the agent the panel runs.
    pub(crate) fn provider(&self, cx: &App) -> ProviderPreferences {
        cx.try_global::<Preferences>()
            .map(|preferences| preferences.agents.provider(self.agent.id))
            .unwrap_or_default()
    }

    /// Find the agents that are installed and turned on, then start one
    /// when `start` is set.
    pub(crate) fn look_for_agents(&mut self, start: bool, cx: &mut Context<Self>) {
        if self.looking.is_some() && !start {
            return;
        }

        let agents = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.agents.clone())
            .unwrap_or_default();
        self.looking = Some(cx.spawn(async move |this, cx| {
            let (installed, cli, path) = cx
                .background_executor()
                .spawn(async move {
                    let path = agent::agent_path();
                    let installed: Vec<&'static Agent> = AGENTS
                        .iter()
                        .filter(|agent| {
                            let provider = agents.provider(agent.id);
                            provider.enabled && agent.launcher(&provider, &path).is_some()
                        })
                        .collect();
                    let cli = agent::find_program("request-eagle-cli", &path);

                    (installed, cli, path)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.looking = None;
                // Run the agent chosen before, or else one that is here,
                // unless a thread with it is on.
                if !installed.contains(&this.agent)
                    && this.session.is_none()
                    && let Some(agent) = installed.first()
                {
                    this.set_agent_name(agent, cx);
                }
                this.briefing.cli = cli;
                this.installed = Some((installed, path));
                if start {
                    this.start(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Start the agent and a thread with it, unless one runs. Its
    /// environment variables come from the OS credential store first.
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.session.is_some() || self.starting.is_some() {
            return;
        }
        let Some((installed, _)) = &self.installed else {
            return;
        };
        if !installed.contains(&self.agent) {
            return;
        }

        let provider = self.provider(cx);
        let environment = if provider.environment.is_empty() {
            Task::ready(Ok(BTreeMap::new()))
        } else {
            preferences::agent_environment(self.agent.id, cx)
        };
        self.failure = None;
        self.starting = Some(cx.spawn(async move |this, cx| {
            let environment = environment.await;
            let _ = this.update(cx, |this, cx| {
                this.starting = None;
                match environment {
                    Ok(environment) => this.connect(provider, environment, cx),
                    Err(error) => {
                        this.queued = None;
                        this.running = false;
                        this.failure = Some(Failure {
                            message: format!(
                                "The agent's environment variables could not be read: {error:#}"
                            ),
                            sign_in: false,
                        });
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn connect(
        &mut self,
        provider: ProviderPreferences,
        environment: BTreeMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        let Some((_, path)) = &self.installed else {
            return;
        };
        let data = &self.briefing.data_directory;
        let additional_directories = if &self.directory == data {
            Vec::new()
        } else {
            vec![data.clone()]
        };
        let Some(launch) = self.agent.launch(
            &provider,
            path.clone(),
            environment,
            self.directory.clone(),
            additional_directories,
        ) else {
            return;
        };

        let (connection, mut events) = Connection::start(launch, cx);
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.handle(event, cx)).is_err() {
                    break;
                }
            }
        });

        self.session = Some(Session {
            connection,
            directory: self.directory.clone(),
            ask_before_tools: provider.ask_before_tools,
            auto_approve: !provider.ask_before_tools,
            ready: None,
            _events: events,
        });
        analytics::capture(
            "agent_thread_started",
            serde_json::json!({ "agent": self.agent.id }),
            cx,
        );
        cx.notify();
    }

    fn handle(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Started {
                embedded_context,
                options,
            } => {
                self.remember_models(&options, cx);
                let bypass = agent::bypass_mode(&options);
                let mut ask = true;
                if let Some(session) = &mut self.session {
                    ask = session.ask_before_tools;
                    session.ready = Some(Ready {
                        embedded_context,
                        confirmed: options.clone(),
                        options,
                    });
                }
                // Unless the settings say to ask, the agent runs tools
                // without asking, as in its Bypass permissions or Full
                // access mode. When they do, an agent set up to start in
                // such a mode starts in one that asks.
                let mode = if ask {
                    self.session
                        .as_ref()
                        .and_then(|session| session.ready.as_ref())
                        .and_then(|ready| agent::prompting_mode(&ready.options))
                } else {
                    bypass
                };
                if let Some((option, value)) = mode {
                    self.set_option(option.0.to_string(), value.0.to_string(), cx);
                }
                if let Some((content, cites)) = self.queued.take() {
                    self.prompt(content, cites);
                }
            }
            Event::Update(SessionUpdate::ConfigOptionUpdate(update)) => {
                self.remember_models(&update.config_options, cx);
                self.set_options(update.config_options);
            }
            Event::Update(update) => {
                let count = self.thread.entries.len();
                if let Some(index) = self.thread.apply(update) {
                    self.changed(index, count, cx);
                    if let Entry::Tool(tool) = &self.thread.entries[index]
                        && tool.status == ToolCallStatus::Completed
                        && !matches!(
                            tool.kind,
                            ToolKind::Read | ToolKind::Search | ToolKind::Think | ToolKind::Fetch
                        )
                    {
                        cx.emit(AgentPanelEvent::FilesChanged);
                    }
                }
            }
            Event::Permission { request, responder } => {
                let count = self.thread.entries.len();
                let index = self.thread.update_tool(&request.tool_call);
                let auto_approve = self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.auto_approve);
                let allow = request
                    .options
                    .iter()
                    .find(|option| option.kind == PermissionOptionKind::AllowOnce)
                    .or_else(|| {
                        request
                            .options
                            .iter()
                            .find(|option| option.kind == PermissionOptionKind::AllowAlways)
                    });
                if auto_approve && let Some(allow) = allow {
                    let _ = responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                            allow.option_id.clone(),
                        )),
                    ));
                    self.changed(index, count, cx);
                    cx.notify();
                    return;
                }
                self.permissions.insert(
                    request.tool_call.tool_call_id.0.to_string(),
                    Permission {
                        options: request.options,
                        responder,
                    },
                );
                self.changed(index, count, cx);
            }
            Event::TurnEnded(result) => {
                self.running = false;
                self.cancel_permissions(cx);
                for index in self.thread.settle_tools() {
                    self.remeasure(index, cx);
                }

                let notice = match result {
                    Ok(StopReason::EndTurn) => None,
                    Ok(StopReason::Cancelled) => Some(("Stopped.".to_owned(), false)),
                    Ok(StopReason::Refusal) => {
                        Some(("The agent declined to answer.".to_owned(), false))
                    }
                    Ok(StopReason::MaxTokens | StopReason::MaxTurnRequests) => {
                        Some(("The agent stopped at its limit.".to_owned(), false))
                    }
                    Ok(_) => None,
                    Err(message) => Some((message, true)),
                };
                if let Some((text, error)) = notice {
                    self.push(Entry::Notice(Notice { text, error }), cx);
                }
                cx.emit(AgentPanelEvent::FilesChanged);
            }
            Event::Options(options) => self.set_options(options),
            Event::OptionRejected(message) => {
                if let Some(ready) = self
                    .session
                    .as_mut()
                    .and_then(|session| session.ready.as_mut())
                {
                    ready.options = ready.confirmed.clone();
                }
                let text = format!("{} kept its setting: {message}", self.agent.name);
                self.push(Entry::Notice(Notice { text, error: true }), cx);
            }
            Event::Failed(failure) => {
                let answering = self.running;
                self.session = None;
                self.running = false;
                self.queued = None;
                self.cancel_permissions(cx);
                for index in self.thread.settle_tools() {
                    self.remeasure(index, cx);
                }
                self.failure = Some(failure);
                if answering {
                    cx.emit(AgentPanelEvent::FilesChanged);
                }
            }
        }

        cx.notify();
    }

    /// Keep the models the agent offers in its settings, where they can be
    /// hidden from the model menu.
    fn remember_models(&self, options: &[SessionConfigOption], cx: &mut Context<Self>) {
        let offered = agent::offered_models(options);
        if offered.is_empty() {
            return;
        }

        let known = self.provider(cx).models;
        let merged = agent::merge_models(&known, offered);
        if merged != known {
            let id = self.agent.id;
            let _ = preferences::update(cx, |preferences| {
                preferences
                    .agents
                    .providers
                    .entry(id.to_owned())
                    .or_default()
                    .models = merged;
            });
        }
    }

    fn set_options(&mut self, options: Vec<SessionConfigOption>) {
        if let Some(ready) = self
            .session
            .as_mut()
            .and_then(|session| session.ready.as_mut())
        {
            ready.confirmed = options.clone();
            ready.options = options;
        }
    }

    /// Keep the list in step with an entry that changed or was added.
    fn changed(&mut self, index: usize, count: usize, cx: &mut Context<Self>) {
        let added = self.thread.entries.len() - count;
        if added > 0 {
            self.scroller
                .update(cx, |scroller, cx| _ = scroller.append(added, cx));
        }
        if index < count {
            self.remeasure(index, cx);
        }
    }

    fn remeasure(&mut self, index: usize, cx: &mut Context<Self>) {
        self.scroller.update(cx, |scroller, cx| {
            _ = scroller.remeasure_items(index..index + 1, cx)
        });
    }

    fn push(&mut self, entry: Entry, cx: &mut Context<Self>) -> usize {
        let index = self.thread.push(entry);
        self.scroller
            .update(cx, |scroller, cx| _ = scroller.append(1, cx));
        index
    }

    /// Quote `cite` in the message being written, where its caret is.
    pub fn add_cite(&mut self, cite: Cite, window: &mut Window, cx: &mut Context<Self>) {
        let id = format!("cite:{}", self.next_cite);
        self.next_cite += 1;
        let label = cite.label();
        let token = InlineToken::new(id.clone(), format!("[{label}]")).with_label(label);

        let focused = self.composer.focus_handle(cx).is_focused(window);
        let inserted = self.composer.update(cx, |composer, cx| {
            // A cite added from a tab goes after what was written so far.
            if !focused {
                let end = composer.text().offset_to_position(composer.text().len());
                composer.set_cursor_position(end, window, cx);
            }
            let start = composer.selected_range().start;
            if composer.value()[..start].ends_with(|character: char| !character.is_whitespace()) {
                composer.replace(" ", window, cx);
            }
            let inserted = composer.replace_with_token(token, window, cx);
            composer.replace(" ", window, cx);
            composer.focus(window, cx);

            inserted
        });

        if inserted.is_ok() {
            self.cites.push((id, cite));
        }
        cx.notify();
    }

    fn on_composer_event(
        &mut self,
        _: &Entity<TextareaState>,
        event: &gpui_kit::component::input::InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            gpui_kit::component::input::InputEvent::PressEnter { shift: false, .. } => {
                self.send(window, cx)
            }
            gpui_kit::component::input::InputEvent::Change => cx.notify(),
            _ => {}
        }
    }

    pub(crate) fn can_send(&self, cx: &App) -> bool {
        !self.running
            && self.failure.is_none()
            && !self.composer.read(cx).value().trim().is_empty()
            && self
                .installed
                .as_ref()
                .is_some_and(|(installed, _)| installed.contains(&self.agent))
    }

    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_send(cx) {
            return;
        }

        let content = self.composer.read(cx).content();
        let cites: Vec<(String, Cite)> = self
            .cites
            .drain(..)
            .filter(|(id, _)| {
                content
                    .tokens()
                    .iter()
                    .any(|span| span.token().id().as_ref() == id)
            })
            .collect();

        let message = cx.new(|cx| {
            let mut message = TextareaState::new(window, cx).auto_grow(1, 200);
            message.set_value(content.clone(), window, cx);
            message
        });
        let index = self.push(
            Entry::User(UserMessage {
                cites: cites.clone(),
            }),
            cx,
        );
        self.messages.insert(index, message);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));

        analytics::capture(
            "agent_message_sent",
            serde_json::json!({
                "agent": self.agent.id,
                "cites": cites.len(),
                "sources": cites.iter().map(|(_, cite)| cite.source.label()).collect::<Vec<_>>(),
            }),
            cx,
        );

        self.running = true;
        match self.session.as_ref().map(|session| session.ready.is_some()) {
            Some(true) => self.prompt(content, cites),
            Some(false) => self.queued = Some((content, cites)),
            None => {
                self.queued = Some((content, cites));
                self.start(cx);
            }
        }
        cx.notify();
    }

    fn prompt(&mut self, content: InputContent, cites: Vec<(String, Cite)>) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(ready) = &session.ready else {
            return;
        };

        let briefing = (!self.briefed).then(|| self.briefing.text());
        self.briefed = true;
        let blocks: Vec<ContentBlock> = prompt(
            &content,
            &cites,
            briefing,
            ready.embedded_context,
            &mut self.cite_files,
        );
        session.connection.send(Command::Prompt(blocks));
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        match &self.session {
            Some(Session {
                connection,
                ready: Some(_),
                ..
            }) => connection.send(Command::Cancel),
            // A message waiting for the agent to start is not sent.
            _ => {
                if self.queued.take().is_some() {
                    self.running = false;
                    self.push(
                        Entry::Notice(Notice {
                            text: "Stopped.".to_owned(),
                            error: false,
                        }),
                        cx,
                    );
                }
            }
        }
        self.cancel_permissions(cx);
        cx.notify();
    }

    /// Answer the agent about running a tool.
    pub(crate) fn decide(&mut self, tool: &str, option: Option<usize>, cx: &mut Context<Self>) {
        let Some(permission) = self.permissions.remove(tool) else {
            return;
        };

        let outcome = match option.and_then(|option| permission.options.get(option)) {
            Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                option.option_id.clone(),
            )),
            None => RequestPermissionOutcome::Cancelled,
        };
        let _ = permission
            .responder
            .respond(RequestPermissionResponse::new(outcome));

        if let Some(index) = self.tool_entry(tool) {
            self.remeasure(index, cx);
        }
        cx.notify();
    }

    /// Questions about tools go unanswered once a turn ends or is stopped.
    fn cancel_permissions(&mut self, cx: &mut Context<Self>) {
        let tools: Vec<String> = self.permissions.keys().cloned().collect();
        for tool in tools {
            self.decide(&tool, None, cx);
        }
    }

    fn tool_entry(&self, id: &str) -> Option<usize> {
        self.thread
            .entries
            .iter()
            .position(|entry| matches!(entry, Entry::Tool(tool) if tool.id.0.as_ref() == id))
    }

    pub(crate) fn set_option(&mut self, id: String, value: String, cx: &mut Context<Self>) {
        let Some(session) = &mut self.session else {
            return;
        };

        // Shown at once; the agent's answer has the options as it set them.
        if let Some(ready) = &mut session.ready
            && let Some(option) = ready
                .options
                .iter_mut()
                .find(|option| option.id.0.as_ref() == id)
            && let agent::acp::schema::v1::SessionConfigKind::Select(select) = &mut option.kind
        {
            select.current_value = value.clone().into();

            // The panel allows tools by itself only while the agent is in
            // a mode that does not ask either.
            if option.category == Some(SessionConfigOptionCategory::Mode) {
                let name = agent::choices(option)
                    .into_iter()
                    .find(|choice| choice.value.0.as_ref() == value)
                    .map(|choice| choice.name.clone())
                    .unwrap_or_default();
                session.auto_approve = !session.ask_before_tools && agent::is_bypass(&value, &name);
            }
        }
        session.connection.send(Command::SetOption {
            id: id.into(),
            value: value.into(),
        });
        cx.notify();
    }

    /// Start again with an empty thread, ending the agent of the last one.
    pub(crate) fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_permissions(cx);
        self.session = None;
        self.starting = None;
        self.failure = None;
        self.running = false;
        self.queued = None;
        self.briefed = false;
        self.cite_files.clear();
        self.thread = Thread::default();
        self.messages.clear();
        self.expanded.clear();
        self.disclosures.clear();
        self.scroller
            .update(cx, |scroller, cx| scroller.reset(0, cx));

        // An agent turned off since the last thread gives way to one that
        // is on.
        if let Some((installed, _)) = &self.installed
            && !installed.contains(&self.agent)
            && let Some(agent) = installed.first().copied()
        {
            self.set_agent_name(agent, cx);
        }
        if self.installed.is_some() {
            self.start(cx);
        } else {
            self.look_for_agents(true, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn select_agent(
        &mut self,
        agent: &'static Agent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if agent == self.agent {
            return;
        }

        self.set_agent_name(agent, cx);
        if let Err(error) = preferences::update(cx, |preferences| {
            preferences.agents.selected = Some(agent.id.to_owned())
        }) {
            eprintln!("Failed to save the agent: {error:#}");
        }
        self.new_thread(window, cx);
    }

    fn set_agent_name(&mut self, agent: &'static Agent, cx: &mut Context<Self>) {
        self.agent = agent;
        cx.notify();
    }

    pub(crate) fn toggle_expanded(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&index) {
            self.expanded.insert(index);
        }
        self.remeasure(index, cx);
        cx.notify();
    }

    pub(crate) fn show_cite(&mut self, id: &str, cx: &mut Context<Self>) {
        let origin = self
            .cites
            .iter()
            .chain(self.thread.entries.iter().flat_map(|entry| match entry {
                Entry::User(message) => message.cites.iter(),
                _ => [].iter(),
            }))
            .find(|(cite, _)| cite == id)
            .map(|(_, cite)| cite.origin.clone());

        if let Some(origin) = origin {
            cx.emit(AgentPanelEvent::ShowCite(origin));
        }
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
