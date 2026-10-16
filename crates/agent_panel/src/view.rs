use agent::acp::schema::v1::{SessionConfigKind, SessionConfigOptionCategory};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    input::{
        InlineTokenClickEvent, InlineTokenContext, InputGroup, InputGroupAddon,
        InputGroupAddonAlignment, InputGroupButton, InputGroupTextarea, InputToken,
    },
    kbd::Kbd,
    menu::{DropdownMenu as _, PopupMenuItem},
    message_scroller::MessageScroller,
    spinner::Spinner,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use tab_ui::{AddToAgent, SendRequest};

use crate::actions::{OpenAgentSettings, ToggleAgentPanel};
use crate::panel::AgentPanel;
use agent::{AGENTS, Agent};

/// How many of the agent's settings the message box offers: its model,
/// reasoning level and mode.
const SHOWN_OPTIONS: usize = 3;

impl AgentPanel {
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let installed: Vec<&'static Agent> = self
            .installed
            .as_ref()
            .map(|(installed, _)| installed.clone())
            .unwrap_or_default();
        let current = self.agent;
        let panel = cx.entity().downgrade();

        // As tall as the tab strip beside it, so their borders meet.
        h_flex()
            .flex_none()
            .h(rems(2.5))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                Button::new("agent-picker")
                    .debug_selector(|| "agent-picker".into())
                    .ghost()
                    .xsmall()
                    .icon(Icon::default().path(current.icon))
                    .label(current.name)
                    .dropdown_caret(true)
                    .disabled(!installed.iter().any(|agent| *agent != current))
                    .accessibility_label("Agent")
                    .dropdown_menu(move |menu, _, _| {
                        installed.iter().fold(menu, |menu, agent| {
                            let agent: &'static Agent = agent;
                            let panel = panel.clone();
                            menu.item(
                                PopupMenuItem::new(agent.name)
                                    .icon(Icon::default().path(agent.icon))
                                    .checked(agent == current)
                                    .on_click(move |_, window, cx| {
                                        let _ = panel.update(cx, |panel, cx| {
                                            panel.select_agent(agent, window, cx)
                                        });
                                    }),
                            )
                        })
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("agent-settings")
                    .debug_selector(|| "agent-settings".into())
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings2)
                    .accessibility_label("Agent settings")
                    .tooltip("Agent Settings")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(OpenAgentSettings.boxed_clone(), cx)
                    }),
            )
            .child(
                Button::new("agent-new-thread")
                    .debug_selector(|| "agent-new-thread".into())
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .accessibility_label("New thread")
                    .tooltip("New Thread")
                    .disabled(self.thread.is_empty() && self.failure.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
            )
            .child(
                Button::new("agent-close")
                    .debug_selector(|| "agent-close".into())
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .accessibility_label("Close the agent panel")
                    .tooltip_with_action("Close", &ToggleAgentPanel, None)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(ToggleAgentPanel.boxed_clone(), cx)
                    }),
            )
    }

    /// Before the first message: what the panel is for, and how to cite.
    fn intro(&self, window: &Window, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let shortcut = Kbd::binding_for_action(&AddToAgent, Some("Workspace"), window);

        v_flex()
            .debug_selector(|| "agent-intro".into())
            .flex_1()
            .min_h_0()
            .justify_center()
            .px_6()
            .gap_3()
            .child(
                Icon::default()
                    .path("icons/sparkles.svg")
                    .size_6()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Ask {} about your API", self.agent.name)),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_1()
                    .gap_y_0p5()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Cite what you select in a request or response with")
                    .when_some(shortcut, |this, shortcut| this.child(shortcut)),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).child(
                "The agent can read your code, and run and edit saved requests and flows.",
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "Works in {}",
                        home_relative(
                            self.session
                                .as_ref()
                                .map_or(&self.directory, |session| &session.directory)
                        )
                    )),
            )
            .when(self.briefing.cli.is_none() && self.installed.is_some(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Install the Request Eagle CLI to let the agent run requests.")
                        .child(
                            Button::new("agent-install-cli")
                                .link()
                                .xsmall()
                                .label("How")
                                .on_click(|_, _, cx| {
                                    cx.open_url("https://github.com/gregor-tokarev/request-eagle/blob/main/docs/cli.md#install")
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    /// None of the agents is on this computer, or turned on in the settings.
    fn no_agents(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let turned_off = cx
            .try_global::<preferences::Preferences>()
            .is_some_and(|preferences| {
                AGENTS
                    .iter()
                    .any(|agent| !preferences.agents.provider(agent.id).enabled)
            });

        v_flex()
            .debug_selector(|| "agent-none-installed".into())
            .flex_1()
            .min_h_0()
            .justify_center()
            .px_6()
            .gap_3()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Install a coding agent"),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).child(
                if turned_off {
                    "The panel runs the agent you already use, with its own sign-in and models. \
                     Install one, or turn one on in the agent settings."
                } else {
                    "The panel runs the agent you already use, with its own sign-in and models. \
                     Install one, then check again."
                },
            ))
            .children(AGENTS.iter().map(|agent| {
                let url = agent.install_url;
                Button::new(SharedString::from(format!("agent-install-{}", agent.id)))
                    .outline()
                    .small()
                    .w_full()
                    .label(agent.name)
                    .icon(IconName::ExternalLink)
                    .on_click(move |_, _, cx| cx.open_url(url))
            }))
            .child(
                Button::new("agent-look-again")
                    .debug_selector(|| "agent-look-again".into())
                    .primary()
                    .small()
                    .w_full()
                    .label("Check Again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.installed = None;
                        this.look_for_agents(true, cx);
                    })),
            )
            .child(
                Button::new("agent-open-settings")
                    .ghost()
                    .small()
                    .w_full()
                    .label("Agent Settings")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(OpenAgentSettings.boxed_clone(), cx)
                    }),
            )
            .into_any_element()
    }

    /// Why the agent stopped, above the message box.
    fn failure_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let failure = self.failure.as_ref()?;
        let theme = cx.theme();
        let (title, detail) = if failure.sign_in {
            (
                format!("Sign in to {}", self.agent.name),
                self.agent.sign_in.to_owned(),
            )
        } else {
            (
                format!("{} stopped", self.agent.name),
                failure.message.clone(),
            )
        };

        Some(
            v_flex()
                .debug_selector(|| "agent-failure".into())
                .mx_3()
                .mb_2()
                .p_3()
                .gap_2()
                .rounded(theme.radius_tokens().lg)
                .border_1()
                .border_color(theme.danger.opacity(0.4))
                .bg(theme.danger.opacity(0.06))
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size_4()
                                .text_color(theme.danger),
                        )
                        .child(title),
                )
                .child(
                    div()
                        .id("agent-failure-detail")
                        .max_h(rems(10.))
                        .overflow_y_scroll()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(detail),
                )
                .child(
                    Button::new("agent-retry")
                        .debug_selector(|| "agent-retry".into())
                        .small()
                        .label("Start Again")
                        .on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
                )
                .into_any_element(),
        )
    }

    fn composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.placeholder != Some(self.agent) {
            let placeholder = format!("Ask {}…", self.agent.name);
            self.composer.update(cx, |composer, cx| {
                composer.set_placeholder(placeholder, window, cx)
            });
            self.placeholder = Some(self.agent);
        }

        let starting = self.starting.is_some()
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.ready.is_none());
        let send = if self.running {
            InputGroupButton::new("agent-stop")
                .icon(Icon::default().path("icons/square.svg"))
                .accessibility_label("Stop")
                .tooltip("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else {
            InputGroupButton::new("agent-send")
                .primary()
                .icon(IconName::ArrowUp)
                .accessibility_label("Send")
                .tooltip("Send")
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
        };
        let send = if self.running || self.can_send(cx) {
            send
        } else {
            send.disabled(true)
        };

        let open = cx.listener(|this, event: &InlineTokenClickEvent, _, cx| {
            this.show_cite(event.token().id(), cx)
        });

        v_flex()
            .flex_none()
            .px_3()
            .pb_3()
            .gap_2()
            .when(starting, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Spinner::new().xsmall())
                        .child(format!("Starting {}…", self.agent.name)),
                )
            })
            .child(
                div().debug_selector(|| "agent-composer".into()).child(
                    InputGroup::new("agent-composer")
                        .input(
                            InputGroupTextarea::new(&self.composer)
                                .aria_label("Message")
                                .token(|token, _, _| cite_token(token))
                                .on_token_click(open),
                        )
                        .addon(
                            InputGroupAddon::new("agent-composer-actions")
                                .align(InputGroupAddonAlignment::BlockEnd)
                                .items_end()
                                // The menus take what room the send button
                                // leaves and wrap onto another line when
                                // their names do not fit on one.
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .flex_wrap()
                                        .gap_0p5()
                                        .children(self.option_buttons(cx)),
                                )
                                .child(send.flex_none()),
                        ),
                ),
            )
    }

    /// The agent's model, reasoning level and mode, as menus, in that order
    /// for every agent.
    fn option_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let provider = self.provider(cx);
        let Some(ready) = self
            .session
            .as_ref()
            .and_then(|session| session.ready.as_ref())
        else {
            return Vec::new();
        };

        let mut options: Vec<_> = ready
            .options
            .iter()
            .filter_map(|option| {
                let order = match option.category {
                    Some(SessionConfigOptionCategory::Model) => 0,
                    Some(SessionConfigOptionCategory::ThoughtLevel) => 1,
                    Some(SessionConfigOptionCategory::Mode) => 2,
                    _ => return None,
                };
                Some((order, option))
            })
            .collect();
        options.sort_by_key(|(order, _)| *order);

        options
            .into_iter()
            .filter_map(|(_, option)| {
                let SessionConfigKind::Select(select) = &option.kind else {
                    return None;
                };
                let current = select.current_value.0.to_string();
                // A default that leaves the model or reasoning level to the
                // agent does not say what runs, so the menus offer only exact
                // ones.
                let model = option.category == Some(SessionConfigOptionCategory::Model);
                let choices: Vec<(String, String)> = agent::choices(option)
                    .into_iter()
                    .map(|choice| {
                        let name = if model {
                            agent::model_name(&choice.value.0, &choice.name)
                        } else {
                            choice.name.clone()
                        };
                        (choice.value.0.to_string(), name)
                    })
                    .filter(|(value, _)| *value == current || !agent::is_unspecified(option, value))
                    .collect();
                // The model button leaves out the provider OpenCode names
                // the model with, which its menu still shows.
                let label = choices
                    .iter()
                    .find(|(value, _)| *value == current)
                    .map_or(current.as_str(), |(_, name)| name.as_str());
                let label = if model {
                    agent::without_provider(label)
                } else {
                    label
                }
                .to_owned();
                // The model menu lists the models in the order the settings
                // give them, without those hidden there.
                let choices = if model {
                    let models = &provider.models;
                    let mut choices: Vec<_> = choices
                        .into_iter()
                        .filter(|(value, _)| {
                            !models
                                .iter()
                                .any(|model| &model.id == value && model.hidden)
                        })
                        .collect();
                    choices.sort_by_key(|(value, _)| {
                        models
                            .iter()
                            .position(|model| &model.id == value)
                            .unwrap_or(usize::MAX)
                    });
                    choices
                } else {
                    choices
                };
                let id = option.id.0.to_string();
                let panel = cx.entity().downgrade();

                Some(
                    Button::new(SharedString::from(format!("agent-option-{id}")))
                        .debug_selector({
                            let id = id.clone();
                            move || format!("agent-option-{id}")
                        })
                        .ghost()
                        .xsmall()
                        .max_w_full()
                        .label(label)
                        .dropdown_caret(true)
                        .accessibility_label(option.name.clone())
                        .tooltip(option.name.clone())
                        .dropdown_menu(move |menu, _, _| {
                            choices.iter().fold(menu, |menu, (value, name)| {
                                let panel = panel.clone();
                                let id = id.clone();
                                let value = value.clone();
                                menu.item(
                                    PopupMenuItem::new(name.clone())
                                        .checked(value == current)
                                        .on_click(move |_, _, cx| {
                                            let _ = panel.update(cx, |panel, cx| {
                                                panel.set_option(id.clone(), value.clone(), cx)
                                            });
                                        }),
                                )
                            })
                        })
                        .into_any_element(),
                )
            })
            .take(SHOWN_OPTIONS)
            .collect()
    }
}

/// A cite in a message, named after what it quotes.
pub(crate) fn cite_token(token: &InlineTokenContext) -> InputToken {
    InputToken::new(token).icon(Icon::default().path("icons/text-quote.svg"))
}

/// A folder as people read it, from their home folder.
pub(crate) fn home_relative(path: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(ToOwned::to_owned))
    {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let none_installed = self
            .installed
            .as_ref()
            .is_some_and(|(installed, _)| installed.is_empty());
        let looking = self.installed.is_none();

        let body = if none_installed {
            self.no_agents(cx)
        } else if looking {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new().small())
                .child("Looking for agents…")
                .into_any_element()
        } else if self.thread.is_empty() {
            self.intro(window, cx)
        } else {
            div()
                .flex_1()
                .min_h_0()
                .child(
                    MessageScroller::new(
                        "agent-thread",
                        self.scroller.clone(),
                        cx.processor(|this, index: usize, window, cx| {
                            this.render_entry(index, window, cx)
                        }),
                    )
                    .with_bottom_fade(cx.theme().sidebar)
                    .with_row_style(StyleRefinement::default().px(rems(0.75)).py(rems(0.375)))
                    .size_full(),
                )
                .into_any_element()
        };
        let failure = self.failure_banner(cx);

        v_flex()
            .debug_selector(|| "agent-panel".into())
            .key_context("AgentPanel")
            // The shortcut that sends a request sends the message here.
            .on_action(cx.listener(|this, _: &SendRequest, window, cx| this.send(window, cx)))
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_l_1()
            .border_color(cx.theme().sidebar_border)
            .child(self.header(cx))
            .child(body)
            .children(failure)
            .when(!none_installed, |this| {
                this.child(self.composer(window, cx))
            })
    }
}
