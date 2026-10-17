use agent::acp::schema::v1::{SessionConfigKind, SessionConfigOptionCategory};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    input::{
        Enter, Escape, IndentInline, InlineTokenClickEvent, InlineTokenContext, InputGroup,
        InputGroupAddon, InputGroupAddonAlignment, InputGroupButton, InputGroupTextarea,
        InputToken, MoveDown, MoveUp,
    },
    kbd::Kbd,
    menu::{DropdownMenu as _, PopupMenuItem},
    message_scroller::MessageScroller,
    select::Select,
    spinner::Spinner,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::protocol_icon;
use tab_ui::{AddToAgent, SendRequest};

use crate::actions::{ChangeModel, NewThread, OpenAgentSettings, ToggleAgentPanel};
use crate::mention::MentionKind;
use crate::mention_menu::MenuKey;
use crate::option_menu::{OptionChoice, OptionMenu};
use crate::panel::AgentPanel;
use crate::thread::Reference;
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
                    .tooltip_with_action("New Thread", &NewThread, Some("AgentComposer"))
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

    /// Before the first message: what the panel is for, and how to cite
    /// and mention.
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
                    // A line of its own that wraps, before the shortcut,
                    // when the panel is narrow.
                    .child(div().min_w_0().child("Cite what you select in a request or response with"))
                    .when_some(shortcut, |this, shortcut| this.child(shortcut)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Type @ to mention a saved request, flow or collection."),
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
                        .flex_wrap()
                        .gap_x_2()
                        .gap_y_0p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        // Wraps, before the link, when the panel is narrow.
                        .child(
                            div()
                                .min_w_0()
                                .child("Install the Request Eagle CLI to let the agent run requests."),
                        )
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
            this.show_reference(event.token().id(), cx)
        });
        let icons = reference_icons(&self.references, cx);

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
                div()
                    .debug_selector(|| "agent-composer".into())
                    .key_context("AgentComposer")
                    .relative()
                    // The keys that move through the mention menu while it
                    // is open, before the message box takes them.
                    .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                        if !action.secondary && !action.shift {
                            this.mention_menu_key(MenuKey::Pick, window, cx)
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                        this.mention_menu_key(MenuKey::Pick, window, cx)
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                        this.mention_menu_key(MenuKey::Up, window, cx)
                    }))
                    .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                        this.mention_menu_key(MenuKey::Down, window, cx)
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.mention_menu_key(MenuKey::Close, window, cx)
                    }))
                    .child(
                        InputGroup::new("agent-composer")
                            .input(
                                InputGroupTextarea::new(&self.composer)
                                    .aria_label("Message")
                                    .token(move |token, _, _| reference_token(token, &icons))
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
                                            .children(self.option_menus(window, cx)),
                                    )
                                    .child(send.flex_none()),
                            ),
                    )
                    .children(self.render_mention_menu(window, cx)),
            )
    }

    /// The agent's model, reasoning level and mode, as menus, in that order
    /// for every agent.
    fn option_menus(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let provider = self.provider(cx);
        let Some(ready) = self
            .session
            .as_ref()
            .and_then(|session| session.ready.as_ref())
        else {
            self.option_menus.clear();
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
                let SessionConfigKind::Select(select) = &option.kind else {
                    return None;
                };
                Some((order, option, select.current_value.0.to_string()))
            })
            .collect();
        options.sort_by_key(|(order, ..)| *order);
        options.truncate(SHOWN_OPTIONS);

        let shown: Vec<_> = options
            .into_iter()
            .map(|(_, option, current)| {
                let model = option.category == Some(SessionConfigOptionCategory::Model);
                // A default that leaves the model or reasoning level to the
                // agent does not say what runs, so the menus offer only exact
                // ones.
                let mut choices: Vec<OptionChoice> = agent::choices(option)
                    .into_iter()
                    .filter(|choice| {
                        *choice.value.0 == *current
                            || !agent::is_unspecified(option, &choice.value.0)
                    })
                    .map(|choice| {
                        let value = choice.value.0.to_string();
                        if !model {
                            return OptionChoice {
                                value,
                                name: choice.name.clone().into(),
                                short_name: None,
                            };
                        }

                        let name = agent::model_name(&value, &choice.name);
                        // The button leaves out the provider OpenCode names
                        // the model with, which the menu still shows.
                        let short_name = self
                            .agent
                            .provider_in_model_names
                            .then(|| agent::without_provider(&name))
                            .filter(|short_name| *short_name != name)
                            .map(|short_name| SharedString::from(short_name.to_owned()));
                        OptionChoice {
                            value,
                            name: name.into(),
                            short_name,
                        }
                    })
                    .collect();
                // The button names the current choice even when the settings
                // hide it from the menu.
                let label: SharedString = choices
                    .iter()
                    .find(|choice| choice.value == current)
                    .map_or_else(
                        || current.clone().into(),
                        |choice| choice.short_name.clone().unwrap_or(choice.name.clone()),
                    );
                // The model menu lists the models in the order the settings
                // give them, without those hidden there.
                if model {
                    let models = &provider.models;
                    choices.retain(|choice| {
                        !models
                            .iter()
                            .any(|model| model.id == choice.value && model.hidden)
                    });
                    choices.sort_by_key(|choice| {
                        models
                            .iter()
                            .position(|model| model.id == choice.value)
                            .unwrap_or(usize::MAX)
                    });
                }

                (
                    option.id.0.to_string(),
                    option.name.clone(),
                    model,
                    choices,
                    current,
                    label,
                )
            })
            .collect();

        self.option_menus
            .retain(|id, _| shown.iter().any(|(shown, ..)| shown == id));

        shown
            .into_iter()
            .map(|(id, name, model, choices, current, label)| {
                let menu = self
                    .option_menus
                    .entry(id.clone())
                    .or_insert_with(|| OptionMenu::new(id.clone(), window, cx));
                menu.show(choices, &current, window, cx);

                let state = menu.state.clone();
                // Focused, or open, as the select shows it with its own
                // border.
                let active = state.read(cx).focus_handle(cx).contains_focused(window, cx);

                div()
                    .debug_selector(move || format!("agent-option-{id}"))
                    .max_w_full()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().transparent)
                    .hover(|this| this.bg(cx.theme().secondary_hover))
                    .when(active, |this| this.focus_ring_style(window, cx))
                    .child(
                        Select::new(&state)
                            .xsmall()
                            .appearance(false)
                            .accessibility_label(name)
                            .placeholder(label)
                            // Wide enough for a model with its provider, and
                            // short enough to stay clear of the panel's top.
                            .menu_width(rems(if model { 20. } else { 12. }))
                            .menu_max_h(rems(18.))
                            // It opens above the message box, so its search
                            // field would move as a search shortens it.
                            .keep_menu_height(true)
                            .search_placeholder("Search"),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

/// The icon of each cite and mention, by the ID of its token.
pub(crate) fn reference_icons(references: &[(String, Reference)], cx: &App) -> Vec<(String, Icon)> {
    references
        .iter()
        .map(|(id, reference)| {
            let icon = match reference {
                Reference::Cite(_) => Icon::default().path("icons/text-quote.svg"),
                Reference::Mention(mention) => match &mention.kind {
                    MentionKind::Request { method, .. } => match *method {
                        "gRPC" | "WS" => protocol_icon(method, cx),
                        _ => protocol_icon("HTTP", cx),
                    },
                    // In the color the sidebar shows flows in.
                    MentionKind::Flow => Icon::default()
                        .path("icons/workflow.svg")
                        .text_color(cx.theme().chart_4),
                    MentionKind::Collection => Icon::default().path("icons/package.svg"),
                },
            };

            (id.clone(), icon)
        })
        .collect()
}

/// A cite in a message, named after what it quotes, or a mention, named
/// after the request, flow or collection, with the icon in `icons`.
pub(crate) fn reference_token(token: &InlineTokenContext, icons: &[(String, Icon)]) -> InputToken {
    let icon = icons
        .iter()
        .find(|(id, _)| id == token.token().id().as_ref())
        .map_or_else(
            || Icon::default().path("icons/text-quote.svg"),
            |(_, icon)| icon.clone(),
        );

    InputToken::new(token).icon(icon)
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
            .on_action(
                cx.listener(|this, _: &ChangeModel, window, cx| this.change_model(window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewThread, window, cx| {
                // As the header's New Thread button, which has nothing to
                // clear before the first message.
                if !this.thread.is_empty() || this.failure.is_some() {
                    this.new_thread(window, cx)
                }
            }))
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
