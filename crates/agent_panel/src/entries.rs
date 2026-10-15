use agent_client_protocol::schema::v1::{
    PermissionOptionKind, PlanEntryStatus, ToolCallStatus, ToolKind,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    input::{InlineTokenClickEvent, Textarea},
    spinner::Spinner,
    text::TextView,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::panel::AgentPanel;
use crate::thread::{Entry, Notice, PlanStep, ToolCall};
use crate::view::cite_token;

/// How many lines of a tool's output show while it is open.
const OUTPUT_LINES: usize = 200;

impl AgentPanel {
    pub(crate) fn render_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(entry) = self.thread.entries.get(index) else {
            return div().into_any_element();
        };

        match entry {
            Entry::User(_) => self.user_message(index, cx),
            Entry::Agent(text) => div()
                .id(("agent-answer", index))
                .w_full()
                .text_sm()
                .child(
                    TextView::markdown(("agent-answer-text", index), text.clone()).selectable(true),
                )
                .into_any_element(),
            Entry::Thought(text) => self.thought(index, text, cx),
            Entry::Tool(tool) => self.tool(index, tool, window, cx),
            Entry::Plan(steps) => plan(steps, cx),
            Entry::Notice(notice) => notice_line(notice, cx),
        }
    }

    fn user_message(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.messages.get(&index) else {
            return div().into_any_element();
        };
        let open = cx.listener(|this, event: &InlineTokenClickEvent, _, cx| {
            this.show_cite(event.token().id(), cx)
        });

        div()
            .id(("agent-message", index))
            .w_full()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius_tokens().lg)
            .bg(cx.theme().muted)
            .text_sm()
            .child(
                Textarea::new(message)
                    .appearance(false)
                    .readonly(true)
                    .token(|token, _, _| cite_token(token))
                    .on_token_click(open),
            )
            .into_any_element()
    }

    fn thought(&self, index: usize, text: &str, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.expanded.contains(&index);
        let theme = cx.theme();

        v_flex()
            .id(("agent-thought", index))
            .w_full()
            .gap_1()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                h_flex()
                    .id(("agent-thought-toggle", index))
                    .gap_1p5()
                    .cursor_pointer()
                    .hover(|this| this.text_color(theme.foreground))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_expanded(index, cx)))
                    .child(Icon::default().path("icons/brain.svg").size_3p5())
                    .child("Thinking")
                    .child(
                        Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size_3(),
                    ),
            )
            .when(expanded, |this| {
                this.child(
                    div()
                        .pl_5()
                        .whitespace_normal()
                        .child(text.trim().to_owned()),
                )
            })
            .into_any_element()
    }

    fn tool(
        &self,
        index: usize,
        tool: &ToolCall,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let expanded = self.expanded.contains(&index);
        let has_output = !tool.output.trim().is_empty();
        let permission = self.permissions.get(tool.id.0.as_ref());

        let status: AnyElement = match tool.status {
            ToolCallStatus::Pending | ToolCallStatus::InProgress if permission.is_none() => {
                Spinner::new().xsmall().into_any_element()
            }
            ToolCallStatus::Failed => Icon::new(IconName::CircleX)
                .size_3p5()
                .text_color(theme.danger)
                .into_any_element(),
            ToolCallStatus::Completed => Icon::new(IconName::Check)
                .size_3p5()
                .text_color(theme.success)
                .into_any_element(),
            _ => Icon::new(IconName::CircleAlert)
                .size_3p5()
                .text_color(theme.warning)
                .into_any_element(),
        };
        let title = if tool.title.is_empty() {
            "Tool".to_owned()
        } else {
            tool.title.clone()
        };

        v_flex()
            .id(("agent-tool", index))
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .id(("agent-tool-row", index))
                    .w_full()
                    .h_6()
                    .px_2()
                    .gap_2()
                    .rounded(theme.radius_tokens().md)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .when(has_output, |this| {
                        this.cursor_pointer()
                            .hover(|this| this.bg(theme.sidebar_accent))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.toggle_expanded(index, cx)),
                            )
                    })
                    .child(div().flex_none().child(status))
                    .child(tool_icon(tool.kind).size_3p5().flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(tool.kind == ToolKind::Execute, |this| {
                                this.font_family(theme.mono_font_family.clone())
                            })
                            .child(title.clone()),
                    )
                    .when(has_output, |this| {
                        this.child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size_3()
                            .flex_none(),
                        )
                    }),
            )
            .when(expanded && has_output, |this| {
                let lines: Vec<&str> = tool.output.lines().collect();
                let hidden = lines.len().saturating_sub(OUTPUT_LINES);
                let shown = lines[..lines.len().min(OUTPUT_LINES)].join("\n");

                this.child(
                    v_flex()
                        .id(("agent-tool-output", index))
                        .ml_6()
                        .max_h(rems(16.))
                        .overflow_y_scroll()
                        .p_2()
                        .gap_1()
                        .rounded(theme.radius_tokens().md)
                        .bg(theme.muted.opacity(0.6))
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .child(div().whitespace_normal().child(shown))
                        .when(hidden > 0, |this| {
                            this.child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("and {hidden} more lines")),
                            )
                        }),
                )
            })
            .when_some(permission, |this, permission| {
                let id = tool.id.0.to_string();

                this.child(
                    v_flex()
                        .debug_selector(|| "agent-permission".into())
                        .ml_6()
                        .p_2()
                        .gap_2()
                        .rounded(theme.radius_tokens().md)
                        .border_1()
                        .border_color(theme.warning.opacity(0.5))
                        .bg(theme.warning.opacity(0.06))
                        .child(
                            div()
                                .text_xs()
                                .child(format!("Allow {} to run this?", self.agent.name)),
                        )
                        .child(
                            h_flex().flex_wrap().gap_1().children(
                                permission
                                    .options
                                    .iter()
                                    .enumerate()
                                    .map(|(choice, option)| {
                                        let id = id.clone();
                                        let allow = matches!(
                                            option.kind,
                                            PermissionOptionKind::AllowOnce
                                                | PermissionOptionKind::AllowAlways
                                        );
                                        Button::new(SharedString::from(format!(
                                            "agent-permission-{id}-{choice}"
                                        )))
                                        .xsmall()
                                        .max_w_full()
                                        .label(option.name.clone())
                                        .tooltip(option.name.clone())
                                        .when(choice == 0 && allow, |button| button.primary())
                                        .when(choice > 0 || !allow, |button| button.outline())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.decide(&id, Some(choice), cx)
                                            }),
                                        )
                                    }),
                            ),
                        ),
                )
            })
            .into_any_element()
    }
}

fn tool_icon(kind: ToolKind) -> Icon {
    match kind {
        ToolKind::Read => Icon::new(IconName::FileText),
        ToolKind::Edit => Icon::default().path("icons/pencil.svg"),
        ToolKind::Delete => Icon::default().path("icons/trash.svg"),
        ToolKind::Move => Icon::default().path("icons/arrow-up-down.svg"),
        ToolKind::Search => Icon::new(IconName::Search),
        ToolKind::Execute => Icon::default().path("icons/terminal.svg"),
        ToolKind::Think => Icon::default().path("icons/brain.svg"),
        ToolKind::Fetch => Icon::new(IconName::Globe),
        _ => Icon::default().path("icons/sparkles.svg"),
    }
}

fn plan(steps: &[PlanStep], cx: &App) -> AnyElement {
    let theme = cx.theme();

    v_flex()
        .w_full()
        .p_2()
        .gap_1()
        .rounded(theme.radius_tokens().md)
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .child(
            h_flex()
                .gap_1p5()
                .text_color(theme.muted_foreground)
                .child(Icon::default().path("icons/list-todo.svg").size_3p5())
                .child("Plan"),
        )
        .children(steps.iter().map(|step| {
            let (mark, color) = match step.status {
                PlanEntryStatus::Completed => ("✓", theme.success),
                PlanEntryStatus::InProgress => ("•", theme.foreground),
                _ => ("○", theme.muted_foreground),
            };

            h_flex()
                .items_start()
                .gap_2()
                .child(div().flex_none().w_3().text_color(color).child(mark))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(step.status == PlanEntryStatus::Completed, |this| {
                            this.text_color(theme.muted_foreground).line_through()
                        })
                        .child(step.text.clone()),
                )
        }))
        .into_any_element()
}

fn notice_line(notice: &Notice, cx: &App) -> AnyElement {
    let theme = cx.theme();

    div()
        .w_full()
        .text_xs()
        .text_color(if notice.error {
            theme.danger
        } else {
            theme.muted_foreground
        })
        .whitespace_normal()
        .child(notice.text.clone())
        .into_any_element()
}
