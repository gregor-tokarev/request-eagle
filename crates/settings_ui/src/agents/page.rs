use std::{collections::BTreeMap, time::Duration};

use agent::{AGENTS, Agent, Check, Status};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    switch::Switch,
    *,
};
use gpui_kit::{prelude::*, *};
use preferences::{Preferences, ProviderPreferences};

use crate::layout::{self, row, section};

/// How long typing in an environment variable waits before it is saved to
/// the credential store.
const ENVIRONMENT_SAVE_DELAY: Duration = Duration::from_millis(600);

/// The coding agents the agent panel runs: whether each is on, how it
/// starts, and which of its models the panel's model menu lists.
pub(crate) struct AgentSettings {
    pub(super) selected: &'static Agent,
    /// The focus of each agent's row, which selects it from the keyboard.
    rows: BTreeMap<&'static str, FocusHandle>,
    /// What the last check of each agent found, by its ID.
    checks: BTreeMap<&'static str, CheckState>,
    command: Entity<InputState>,
    config_directory: Entity<InputState>,
    variables: Vec<Variable>,
    /// Whether the selected agent's variables are read from the credential
    /// store yet.
    environment: EnvironmentState,
    environment_save: Option<Task<()>>,
    /// Whether variables are being written to the credential store, which
    /// can wait for it to be unlocked.
    saving_environment: bool,
    /// Inputs are being filled for the agent just selected, which is not an
    /// edit to save.
    filling: bool,
    pub(super) error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

enum CheckState {
    Checking(#[allow(dead_code)] Task<()>),
    Done(Check),
}

enum EnvironmentState {
    Loading(#[allow(dead_code)] Task<()>),
    Loaded,
    Failed(String),
}

/// An environment variable being edited.
struct Variable {
    name: Entity<InputState>,
    value: Entity<InputState>,
    _subscriptions: [Subscription; 2],
}

impl AgentSettings {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let command = cx.new(|cx| InputState::new(window, cx));
        let config_directory = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe(&command, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_runtime(cx);
                }
            }),
            cx.subscribe(&config_directory, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_runtime(cx);
                }
            }),
            // Models arrive as the panel's agents start.
            cx.observe_global::<Preferences>(|_, cx| cx.notify()),
        ];

        let mut settings = Self {
            selected: &AGENTS[0],
            rows: AGENTS
                .iter()
                .map(|agent| (agent.id, cx.focus_handle()))
                .collect(),
            checks: BTreeMap::new(),
            command,
            config_directory,
            variables: Vec::new(),
            environment: EnvironmentState::Loaded,
            environment_save: None,
            saving_environment: false,
            filling: false,
            error: None,
            _subscriptions: subscriptions,
        };
        settings.fill(window, cx);

        settings
    }

    /// When the page is first shown, select the agent the panel runs and
    /// check the agents that are on.
    pub(crate) fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.checks.is_empty() {
            return;
        }

        if let Some(agent) = cx
            .global::<Preferences>()
            .agents
            .selected
            .as_deref()
            .and_then(Agent::find)
        {
            self.select(agent, window, cx);
        }
        self.check_all(cx);
    }

    pub(super) fn provider(&self, cx: &App) -> ProviderPreferences {
        cx.global::<Preferences>().agents.provider(self.selected.id)
    }

    /// Change the selected agent's settings.
    pub(super) fn update_provider(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ProviderPreferences),
    ) {
        let id = self.selected.id;
        self.error = preferences::update(cx, |preferences| {
            change(
                preferences
                    .agents
                    .providers
                    .entry(id.to_owned())
                    .or_default(),
            );
        })
        .err()
        .map(|error| format!("Could not save the agent settings: {error}"));
        cx.notify();
    }

    fn select(&mut self, agent: &'static Agent, window: &mut Window, cx: &mut Context<Self>) {
        if agent == self.selected {
            return;
        }

        self.selected = agent;
        self.fill(window, cx);
        cx.notify();
    }

    /// Show the selected agent's settings in the inputs.
    fn fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.selected;
        let provider = self.provider(cx);

        self.filling = true;
        self.command.update(cx, |input, cx| {
            input.set_placeholder(agent.default_command(), window, cx);
            input.set_value(provider.command.clone(), window, cx);
        });
        self.config_directory.update(cx, |input, cx| {
            input.set_placeholder(agent.default_config_directory, window, cx);
            input.set_value(provider.config_directory.clone(), window, cx);
        });
        self.filling = false;

        // Edits to the agent selected before still save.
        if let Some(save) = self.environment_save.take() {
            save.detach();
        }
        self.variables.clear();
        if provider.environment.is_empty() {
            self.environment = EnvironmentState::Loaded;
            return;
        }

        let read = preferences::agent_environment(agent.id, cx);
        self.environment = EnvironmentState::Loading(cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.selected != agent {
                    return;
                }
                match result {
                    Ok(mut values) => {
                        // A name without a value in the store keeps its row.
                        for name in &provider.environment {
                            values.entry(name.clone()).or_default();
                        }
                        for (name, value) in values {
                            this.push_variable(name, value, window, cx);
                        }
                        this.environment = EnvironmentState::Loaded;
                    }
                    Err(error) => {
                        this.environment = EnvironmentState::Failed(format!(
                            "The environment variables could not be read: {error:#}"
                        ));
                    }
                }
                cx.notify();
            });
        }));
    }

    fn save_runtime(&mut self, cx: &mut Context<Self>) {
        if self.filling {
            return;
        }

        let command = self.command.read(cx).value().trim().to_owned();
        let config_directory = self.config_directory.read(cx).value().trim().to_owned();
        self.update_provider(cx, |provider| {
            provider.command = command;
            provider.config_directory = config_directory;
        });
    }

    fn push_variable(
        &mut self,
        name: String,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("NAME")
                .default_value(name)
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .masked(true)
                .default_value(value)
        });
        let subscriptions = [&name, &value].map(|input| {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_environment(cx);
                }
            })
        });

        self.variables.push(Variable {
            name,
            value,
            _subscriptions: subscriptions,
        });
    }

    /// Save the variables a moment after the last edit, since each save
    /// writes to the credential store.
    fn save_environment(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.environment, EnvironmentState::Loaded) {
            return;
        }

        let agent = self.selected.id;
        let variables: BTreeMap<String, String> = self
            .variables
            .iter()
            .map(|variable| {
                (
                    variable.name.read(cx).value().trim().to_owned(),
                    variable.value.read(cx).value().to_string(),
                )
            })
            .filter(|(name, _)| !name.is_empty())
            .collect();

        self.environment_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENVIRONMENT_SAVE_DELAY).await;
            let save = cx.update(|cx| preferences::update_agent_environment(agent, variables, cx));
            let _ = this.update(cx, |this, cx| {
                this.saving_environment = true;
                cx.notify();
            });
            let result = save.await;
            let _ = this.update(cx, |this, cx| {
                this.saving_environment = false;
                this.error = result
                    .err()
                    .map(|error| format!("Could not save the environment variables: {error:#}"));
                cx.notify();
            });
        }));
    }

    /// Check every agent that is on, as the panel would start it.
    fn check_all(&mut self, cx: &mut Context<Self>) {
        let agents = cx.global::<Preferences>().agents.clone();
        for agent in &AGENTS {
            if agents.provider(agent.id).enabled {
                self.check(agent, cx);
            } else {
                self.checks.remove(agent.id);
            }
        }
    }

    fn check(&mut self, agent: &'static Agent, cx: &mut Context<Self>) {
        let provider = cx.global::<Preferences>().agents.provider(agent.id);
        let environment = if provider.environment.is_empty() {
            Task::ready(Ok(BTreeMap::new()))
        } else {
            preferences::agent_environment(agent.id, cx)
        };
        let directory = std::env::home_dir().unwrap_or_else(std::env::temp_dir);

        let task = cx.spawn(async move |this, cx| {
            // An agent whose variables cannot be read is not started
            // without them, as the panel does not start it either.
            let check = match environment.await {
                Ok(environment) => {
                    let check =
                        cx.update(|cx| agent::check(agent, provider, environment, directory, cx));
                    check.await
                }
                Err(error) => Check {
                    version: None,
                    status: Status::Failed(format!(
                        "The agent's environment variables could not be read: {error:#}"
                    )),
                    models: Vec::new(),
                },
            };

            let _ = this.update(cx, |this, cx| {
                if !check.models.is_empty() {
                    let known = cx.global::<Preferences>().agents.provider(agent.id).models;
                    let merged = agent::merge_models(&known, check.models.clone());
                    if merged != known {
                        let _ = preferences::update(cx, |preferences| {
                            preferences
                                .agents
                                .providers
                                .entry(agent.id.to_owned())
                                .or_default()
                                .models = merged;
                        });
                    }
                }
                this.checks.insert(agent.id, CheckState::Done(check));
                cx.notify();
            });
        });
        self.checks.insert(agent.id, CheckState::Checking(task));
        cx.notify();
    }

    fn checking(&self) -> bool {
        self.checks
            .values()
            .any(|check| matches!(check, CheckState::Checking(_)))
    }

    fn version(&self, agent: &Agent) -> Option<String> {
        match self.checks.get(agent.id) {
            Some(CheckState::Done(check)) => {
                check.version.as_ref().map(|version| format!("v{version}"))
            }
            _ => None,
        }
    }

    /// The agent's state in a few words, and whether it needs attention.
    fn status(&self, agent: &Agent, cx: &App) -> (SharedString, bool) {
        if !cx.global::<Preferences>().agents.provider(agent.id).enabled {
            return ("Off".into(), false);
        }

        match self.checks.get(agent.id) {
            None => ("Not checked".into(), false),
            Some(CheckState::Checking(_)) => ("Checking…".into(), false),
            Some(CheckState::Done(check)) => match &check.status {
                Status::Ready => ("Ready".into(), false),
                Status::SignInNeeded => ("Sign in needed".into(), true),
                Status::NotInstalled => ("Not installed".into(), true),
                Status::Failed(_) => ("Couldn't start".into(), true),
            },
        }
    }

    fn list(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let rows: Vec<AnyElement> = AGENTS
            .iter()
            .map(|agent| {
                let selected = agent == self.selected;
                let enabled = cx.global::<Preferences>().agents.provider(agent.id).enabled;
                let (status, attention) = self.status(agent, cx);
                let version = self.version(agent);
                let focus = &self.rows[agent.id];
                let focus_visible = focus.is_focused(window) && window.last_input_was_keyboard();

                // The row selects the agent from the pointer or the keyboard;
                // its switch turns the agent on and off.
                let select = gpui_kit::base::Button::new(SharedString::from(format!(
                    "agent-provider-select-{}",
                    agent.id
                )))
                .track_focus(focus)
                .accessibility_label(agent.name)
                .selected(selected)
                .flex_1()
                .min_w_0()
                .justify_start()
                .items_start()
                .gap_3()
                .pl_3()
                .py_3()
                .cursor_pointer()
                .rounded(theme.radius_tokens().md)
                .when(focus_visible, |this| this.focus_ring_style(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.select(agent, window, cx)))
                .child(
                    Icon::default()
                        .path(agent.icon)
                        .size_4()
                        .mt_0p5()
                        .flex_none()
                        .text_color(if enabled {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            h_flex()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_ellipsis()
                                        .child(agent.name),
                                )
                                .when_some(version, |this, version| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .font_family(theme.mono_font_family.clone())
                                            .text_color(theme.muted_foreground)
                                            .child(version),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if attention {
                                    theme.warning
                                } else {
                                    theme.muted_foreground
                                })
                                .child(status),
                        ),
                );

                h_flex()
                    .id(SharedString::from(format!("agent-provider-{}", agent.id)))
                    .debug_selector(move || format!("agent-provider-{}", agent.id))
                    .w_full()
                    .items_start()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .when(selected, |this| this.bg(theme.muted))
                    .hover(|this| this.bg(theme.muted.opacity(0.6)))
                    .child(select)
                    .child(
                        div().flex_none().pt_3().pr_3().child(
                            Switch::new(SharedString::from(format!("agent-enabled-{}", agent.id)))
                                .accessibility_label(format!("Turn on {}", agent.name))
                                .checked(enabled)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    let checked = *checked;
                                    this.error = preferences::update(cx, |preferences| {
                                        preferences
                                            .agents
                                            .providers
                                            .entry(agent.id.to_owned())
                                            .or_default()
                                            .enabled = checked;
                                    })
                                    .err()
                                    .map(|error| {
                                        format!("Could not save the agent settings: {error}")
                                    });
                                    if checked {
                                        this.check(agent, cx);
                                    } else {
                                        this.checks.remove(agent.id);
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex().w_full().children(rows)
    }

    /// The selected agent's name, version and what its check found.
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let agent = self.selected;
        let theme = cx.theme();
        let check = match self.checks.get(agent.id) {
            Some(CheckState::Done(check)) => Some(&check.status),
            _ => None,
        };
        let enabled = self.provider(cx).enabled;

        let detail: Option<AnyElement> = match check {
            Some(Status::NotInstalled) if enabled => Some(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} isn't on this computer.", agent.name))
                    .child(
                        Button::new("agent-install")
                            .link()
                            .label("Install")
                            .on_click(move |_, _, cx| cx.open_url(agent.install_url)),
                    )
                    .into_any_element(),
            ),
            Some(Status::SignInNeeded) if enabled => Some(
                div()
                    .text_color(theme.muted_foreground)
                    .child(agent.sign_in)
                    .into_any_element(),
            ),
            Some(Status::Failed(message)) if enabled => Some(
                div()
                    .id("agent-check-error")
                    .max_h(rems(8.))
                    .overflow_y_scroll()
                    .text_xs()
                    .text_color(theme.danger)
                    .whitespace_normal()
                    .child(message.clone())
                    .into_any_element(),
            ),
            _ => None,
        };

        v_flex()
            .w_full()
            .gap_2()
            .pb_4()
            .child(
                h_flex()
                    .gap_3()
                    .child(Icon::default().path(agent.icon).size_5())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(agent.name),
                    )
                    .child(div().flex_1())
                    .when_some(self.version(agent), |this, version| {
                        this.child(
                            div()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(version),
                        )
                    }),
            )
            .children(detail)
    }

    fn permissions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let agent = self.selected;
        let ask = self.provider(cx).ask_before_tools;

        section("Permissions").child(row(
            "Ask before running tools",
            match (ask, agent.bypass_mode) {
                (true, _) => format!(
                    "{} asks before it runs a command or changes a file.",
                    agent.name
                ),
                (false, Some(mode)) => format!(
                    "{} starts in its {mode} mode and runs commands and changes files without \
                     asking.",
                    agent.name
                ),
                (false, None) => format!(
                    "{} runs commands and changes files without asking: the panel allows what \
                     it asks about.",
                    agent.name
                ),
            },
            Switch::new("agent-ask-before-tools")
                .accessibility_label("Ask before running tools")
                .checked(ask)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    let checked = *checked;
                    this.update_provider(cx, |provider| provider.ask_before_tools = checked);
                })),
            cx,
        ))
    }

    fn runtime(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let agent = self.selected;
        let field = |title: &'static str, description: String, input: &Entity<InputState>| {
            v_flex()
                .w_full()
                .py_4()
                .gap_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
                .child(Input::new(input).aria_label(title).w_full())
        };

        section("Runtime")
            .child(field(
                "Command",
                "The command that starts the agent's ACP server. Leave it empty for the default."
                    .to_owned(),
                &self.command,
            ))
            .child(field(
                "Config folder",
                format!(
                    "Where {} keeps its settings and sign-in, set with {}. Leave it empty for \
                     its own.",
                    agent.name, agent.config_variable
                ),
                &self.config_directory,
            ))
    }

    fn environment(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let keyring = if cfg!(target_os = "linux") {
            "your desktop keyring"
        } else if cfg!(windows) {
            "Windows Credential Manager"
        } else {
            "macOS Keychain"
        };

        let body: AnyElement = match &self.environment {
            EnvironmentState::Loading(_) => h_flex()
                .gap_2()
                .text_color(theme.muted_foreground)
                .child(Spinner::new().small())
                .child("Reading the variables…")
                .into_any_element(),
            EnvironmentState::Failed(error) => div()
                .text_color(theme.danger)
                .child(error.clone())
                .into_any_element(),
            EnvironmentState::Loaded => v_flex()
                .w_full()
                .gap_2()
                .children(self.variables.iter().enumerate().map(|(index, variable)| {
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div().w(rems(12.)).flex_none().child(
                                Input::new(&variable.name)
                                    .aria_label("Variable name")
                                    .w_full(),
                            ),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&variable.value)
                                    .aria_label("Variable value")
                                    .mask_toggle()
                                    .w_full(),
                            ),
                        )
                        .child(
                            Button::new(("agent-variable-remove", index))
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .accessibility_label("Remove variable")
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if index < this.variables.len() {
                                        this.variables.remove(index);
                                        this.save_environment(cx);
                                        cx.notify();
                                    }
                                })),
                        )
                }))
                .child(
                    h_flex().child(
                        Button::new("agent-variable-add")
                            .debug_selector(|| "agent-variable-add".into())
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add Variable")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.push_variable(String::new(), String::new(), window, cx);
                                if let Some(variable) = this.variables.last() {
                                    variable
                                        .name
                                        .update(cx, |input, cx| input.focus(window, cx));
                                }
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element(),
        };

        section("Environment").child(
            v_flex()
                .w_full()
                .py_4()
                .gap_3()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().font_weight(FontWeight::MEDIUM).child("Variables"))
                        .when(self.saving_environment, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Saving to {keyring}…")),
                            )
                        }),
                )
                .child(div().text_color(theme.muted_foreground).child(format!(
                    "API keys, base URLs and other settings {} runs with. Their values are \
                     saved in {keyring}.",
                    self.selected.name
                )))
                .child(body),
        )
    }
}

impl Render for AgentSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = layout::is_narrow(window);
        let checking = self.checking();

        let detail = v_flex()
            .flex_1()
            .min_w_0()
            .p_5()
            .gap_6()
            .child(self.header(cx))
            .child(self.permissions(cx))
            .child(self.runtime(cx))
            .child(self.environment(cx))
            .child(self.models(cx));
        let list = self.list(window, cx);
        let theme = cx.theme();

        v_flex()
            .debug_selector(|| "agent-settings".into())
            .w_full()
            .max_w(layout::PAGE_WIDTH)
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Agents"),
                            )
                            .child(
                                Button::new("agent-check-all")
                                    .debug_selector(|| "agent-check-all".into())
                                    .ghost()
                                    .small()
                                    .icon(Icon::default().path("icons/refresh-cw.svg"))
                                    .label(if checking {
                                        "Checking…"
                                    } else {
                                        "Check Again"
                                    })
                                    .loading(checking)
                                    .disabled(checking)
                                    .on_click(cx.listener(|this, _, _, cx| this.check_all(cx))),
                            ),
                    )
                    .child(div().text_color(theme.muted_foreground).child(
                        "The coding agents the agent panel runs. Each keeps its own sign-in, \
                         models and tools. Changes save automatically and apply to new threads.",
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .when(narrow, |this| this.flex_col())
                    .rounded(theme.radius_tokens().lg)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(
                        div()
                            .when(narrow, |this| this.w_full())
                            .when(!narrow, |this| {
                                this.w(rems(16.))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(theme.border)
                            })
                            .child(list),
                    )
                    .child(detail),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_color(cx.theme().danger).child(error))
            })
    }
}
