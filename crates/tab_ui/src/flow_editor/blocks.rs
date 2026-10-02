use flow::{Block, BlockKind, BlockType};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, spinner::Spinner, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_color;

use super::{
    FlowEditor,
    editor::Layout,
    geometry::{BASE_REM, HEADER, PORT, ROW, port_offset},
    preview,
    run::BlockStatus,
};

/// Below this zoom, text would be too small to read, so blocks are drawn as
/// shapes only. Large flows stay fast when the whole flow is in view.
const DETAIL_ZOOM: f32 = 0.5;

/// Canvas pixels as rems, which the zoomed canvas scales.
fn units(value: f32) -> Rems {
    rems(value / BASE_REM)
}

pub(super) fn icon(block_type: BlockType) -> &'static str {
    match block_type {
        BlockType::Start => "icons/play.svg",
        BlockType::HttpRequest => "icons/send-horizontal.svg",
        BlockType::Evaluate => "icons/square-function.svg",
        BlockType::If => "icons/split.svg",
        BlockType::Condition => "icons/git-branch.svg",
        BlockType::Validate => "icons/shield-check.svg",
        BlockType::Delay => "icons/timer.svg",
        BlockType::Or => "icons/merge.svg",
        BlockType::Repeat => "icons/repeat.svg",
        BlockType::For => "icons/list-ordered.svg",
        BlockType::Collect => "icons/combine.svg",
        BlockType::Display => "icons/monitor.svg",
        BlockType::Log => "icons/scroll-text.svg",
        BlockType::String => "icons/type.svg",
        BlockType::Number => "icons/hash.svg",
        BlockType::Boolean => "icons/toggle-left.svg",
        BlockType::Null => "icons/circle-off.svg",
        BlockType::Now => "icons/clock.svg",
        BlockType::Date => "icons/calendar.svg",
        BlockType::Select => "icons/mouse-pointer-click.svg",
        BlockType::Record => "icons/braces.svg",
        BlockType::List => "icons/list.svg",
        BlockType::Template => "icons/file-text.svg",
        BlockType::SetVariable => "icons/log-in.svg",
        BlockType::GetVariable => "icons/log-out.svg",
        BlockType::Output => "icons/flag.svg",
        BlockType::Note => "icons/sticky-note.svg",
    }
}

/// The color of a block's kind of work, shared by its icon and the picker.
pub(super) fn color(block_type: BlockType, cx: &App) -> Hsla {
    let theme = cx.theme();

    match block_type {
        BlockType::Start | BlockType::Output => theme.success,
        BlockType::HttpRequest => method_color("HTTP", cx),
        BlockType::Evaluate
        | BlockType::If
        | BlockType::Condition
        | BlockType::Validate
        | BlockType::Or => theme.warning,
        BlockType::Repeat | BlockType::For | BlockType::Collect | BlockType::Delay => theme.info,
        BlockType::Display | BlockType::Log => theme.chart_4,
        BlockType::Note => theme.muted_foreground,
        _ => theme.chart_2,
    }
}

impl FlowEditor {
    pub(super) fn block_element(
        &self,
        block: &Block,
        layout: &Layout,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let block_type = block.kind.block_type();
        let selected = self.selection.contains(&block.id);
        let status = self.run.blocks.get(&block.id);
        let origin = self.viewport.to_view(layout.bounds.origin, self.rem);
        let id = block.id.clone();
        let rows = layout.inputs.len().max(layout.outputs.len());
        let note = block_type == BlockType::Note;
        let connecting = self.connecting_port();

        if self.viewport.zoom < DETAIL_ZOOM {
            return self.block_outline(block, layout, cx);
        }

        div()
            .id(SharedString::from(format!("flow-block-{}", block.id)))
            .debug_selector({
                let id = block.id.clone();
                move || format!("flow-block-{id}")
            })
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(units(layout.bounds.size.width))
            .h(units(layout.bounds.size.height))
            .flex()
            .flex_col()
            .rounded(theme.radius_tokens().lg)
            .border_1()
            .border_color(if selected { theme.ring } else { theme.border })
            .bg(if note {
                theme.warning.opacity(0.12)
            } else {
                theme.popover
            })
            .shadow_sm()
            .text_color(theme.popover_foreground)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.press(event, Some(id.clone()), window, cx);
                }),
            )
            .child(self.block_header(block, status, cx))
            .children((0..rows).map(|row| {
                h_flex()
                    .flex_none()
                    .h(units(ROW))
                    .px(units(12.))
                    .gap(units(8.))
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_color(theme.muted_foreground)
                            .children(layout.inputs.get(row).map(port_label)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_right()
                            .children(layout.outputs.get(row).map(|name| {
                                let sent = status.is_some_and(|status| status.sent(name));
                                div()
                                    .when(!sent, |this| this.text_color(theme.muted_foreground))
                                    .when(sent, |this| this.font_weight(FontWeight::MEDIUM))
                                    .child(port_label(name))
                            })),
                    )
            }))
            .child(self.block_body(block, layout, status, cx))
            .children(layout.inputs.iter().enumerate().map(|(index, name)| {
                let connected = self.flow.connection_into(&block.id, name).is_some();
                let target = connecting.as_ref().is_some_and(|port| {
                    !port.output && port.block == block.id && port.port == name.as_ref()
                });
                port_dot(0., port_offset(index), connected, target, cx)
            }))
            .children(layout.outputs.iter().enumerate().map(|(index, name)| {
                let connected = self.flow.connections.iter().any(|connection| {
                    connection.from == block.id && connection.output == name.as_ref()
                });
                let target = connecting.as_ref().is_some_and(|port| {
                    port.output && port.block == block.id && port.port == name.as_ref()
                });
                port_dot(
                    layout.bounds.size.width,
                    port_offset(index),
                    connected,
                    target,
                    cx,
                )
            }))
            .into_any_element()
    }

    /// A block without its text, for when the canvas is zoomed far out.
    fn block_outline(&self, block: &Block, layout: &Layout, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let block_type = block.kind.block_type();
        let selected = self.selection.contains(&block.id);
        let status = self.run.blocks.get(&block.id);
        let origin = self.viewport.to_view(layout.bounds.origin, self.rem);
        let id = block.id.clone();
        let state = status.map(|status| {
            if status.running {
                theme.info
            } else if status.failed() {
                theme.danger
            } else {
                theme.success
            }
        });

        div()
            .id(SharedString::from(format!("flow-block-{}", block.id)))
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(units(layout.bounds.size.width))
            .h(units(layout.bounds.size.height))
            .rounded(theme.radius_tokens().lg)
            .border_1()
            .border_color(if selected { theme.ring } else { theme.border })
            .bg(theme.popover)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.press(event, Some(id.clone()), window, cx);
                }),
            )
            .child(
                h_flex()
                    .h(units(HEADER))
                    .px(units(12.))
                    .justify_end()
                    .rounded_t(theme.radius_tokens().lg)
                    .bg(color(block_type, cx).opacity(0.3))
                    .children(state.map(|state| div().size(units(12.)).rounded_full().bg(state))),
            )
            .children(layout.inputs.iter().enumerate().map(|(index, name)| {
                let connected = self.flow.connection_into(&block.id, name).is_some();
                port_dot(0., port_offset(index), connected, false, cx)
            }))
            .children(layout.outputs.iter().enumerate().map(|(index, _)| {
                port_dot(
                    layout.bounds.size.width,
                    port_offset(index),
                    false,
                    false,
                    cx,
                )
            }))
            .into_any_element()
    }

    fn block_header(
        &self,
        block: &Block,
        status: Option<&BlockStatus>,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let block_type = block.kind.block_type();

        h_flex()
            .flex_none()
            .h(units(HEADER))
            .px(units(12.))
            .gap(units(8.))
            .border_b_1()
            .border_color(theme.border)
            .child(
                Icon::default()
                    .path(icon(block_type))
                    .size(units(16.))
                    .flex_none()
                    .text_color(color(block_type, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .child(SharedString::from(block.title().to_owned())),
            )
            .when_some(status, |this, status| {
                if status.running {
                    this.child(Spinner::new().xsmall().color(theme.info))
                } else if status.failed() {
                    this.child(
                        Icon::default()
                            .path("icons/circle-alert.svg")
                            .size(units(14.))
                            .text_color(theme.danger),
                    )
                } else if status.runs > 0 {
                    this.child(
                        h_flex()
                            .flex_none()
                            .gap(units(4.))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .when(status.runs > 1, |this| {
                                this.child(format!("×{}", status.runs))
                            })
                            .child(
                                Icon::new(IconName::Check)
                                    .size(units(14.))
                                    .text_color(theme.success),
                            ),
                    )
                } else {
                    this
                }
            })
    }

    fn block_body(
        &self,
        block: &Block,
        layout: &Layout,
        status: Option<&BlockStatus>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let body = div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .px(units(12.))
            .py(units(6.))
            .text_xs()
            .text_color(theme.muted_foreground);

        if let Some(error) = status
            .and_then(|status| status.last.as_ref())
            .and_then(|run| run.error.clone())
        {
            return body
                .text_color(theme.danger)
                .child(SharedString::from(error))
                .into_any_element();
        }

        let code = |text: &str| {
            div()
                .font_family(theme.mono_font_family.clone())
                .line_clamp(2)
                .text_ellipsis()
                .child(SharedString::from(text.to_owned()))
        };

        let content = match &block.kind {
            BlockKind::Start { input } => {
                if input.trim().is_empty() {
                    div().child("Sends the run's input").into_any_element()
                } else {
                    code(input).into_any_element()
                }
            }
            BlockKind::HttpRequest { request } => match &layout.request {
                Some((method, name)) => h_flex()
                    .gap(units(6.))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(method_color(method, cx))
                            .child(*method),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_color(theme.foreground)
                            .child(name.clone()),
                    )
                    .into_any_element(),
                None if request.is_empty() => div()
                    .text_color(theme.warning)
                    .child("Choose a request")
                    .into_any_element(),
                None => div()
                    .text_color(theme.danger)
                    .child("The request is no longer saved")
                    .into_any_element(),
            },
            BlockKind::Evaluate { expression, .. } => code(expression).into_any_element(),
            BlockKind::If { condition, .. } => code(condition).into_any_element(),
            BlockKind::Condition { conditions, .. } => div()
                .child(format!(
                    "{} condition{}, then Default",
                    conditions.len(),
                    if conditions.len() == 1 { "" } else { "s" }
                ))
                .into_any_element(),
            BlockKind::Validate { .. } => div().child("Checks a JSON Schema").into_any_element(),
            BlockKind::Delay { milliseconds } => div()
                .child(format!("Waits {milliseconds} ms"))
                .into_any_element(),
            BlockKind::Or => div().child("Sends what arrives").into_any_element(),
            BlockKind::Repeat => div().child("Sends each index").into_any_element(),
            BlockKind::For => div().child("Sends each item").into_any_element(),
            BlockKind::Collect => div().child("Gathers the loop's results").into_any_element(),
            BlockKind::Display { .. } => {
                return body
                    .text_color(theme.foreground)
                    .child(match status.and_then(|status| status.display.as_ref()) {
                        Some((_, display)) => display_element(display, cx),
                        None => div()
                            .text_color(theme.muted_foreground)
                            .child("Run the flow to see its data")
                            .into_any_element(),
                    })
                    .into_any_element();
            }
            BlockKind::Log => div().child("Writes to the run log").into_any_element(),
            BlockKind::String { value } => code(&format!("\"{value}\"")).into_any_element(),
            BlockKind::Number { value } => code(&value.to_string()).into_any_element(),
            BlockKind::Boolean { value } => code(&value.to_string()).into_any_element(),
            BlockKind::Null => code("null").into_any_element(),
            BlockKind::Now => div().child("The time it runs").into_any_element(),
            BlockKind::Date { value } => code(value).into_any_element(),
            BlockKind::Select { path } => {
                if path.trim().is_empty() {
                    div().child("All of the data").into_any_element()
                } else {
                    code(path).into_any_element()
                }
            }
            BlockKind::Record { fields } => code(&format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|field| field.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .into_any_element(),
            BlockKind::List { items } => div()
                .child(format!(
                    "{} item{}",
                    items.len(),
                    if items.len() == 1 { "" } else { "s" }
                ))
                .into_any_element(),
            BlockKind::Template { template, .. } => code(template).into_any_element(),
            BlockKind::SetVariable { name } | BlockKind::GetVariable { name } => {
                code(name).into_any_element()
            }
            BlockKind::Output { .. } => div().child("Returns the run's results").into_any_element(),
            BlockKind::Note { text } => {
                return body
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(SharedString::from(text.clone()))
                    .into_any_element();
            }
        };

        body.child(content)
            .when_some(
                status
                    .and_then(|status| status.last.as_ref())
                    .and_then(|run| run.notice.clone()),
                |this, notice| {
                    this.child(
                        div()
                            .text_color(theme.warning)
                            .text_ellipsis()
                            .child(SharedString::from(notice)),
                    )
                },
            )
            .into_any_element()
    }
}

/// Ports a block type names itself read as words; variables, fields and
/// outputs that people name keep their names.
pub(super) fn port_label(name: &SharedString) -> SharedString {
    const NAMED: [&str; 19] = [
        "data", "send", "success", "fail", "result", "then", "else", "default", "pass", "first",
        "second", "count", "start", "list", "item", "index", "finish", "value", "record",
    ];

    for (prefix, label) in [("condition", "Condition"), ("item", "Item")] {
        if let Some(number) = name.strip_prefix(prefix)
            && number.parse::<usize>().is_ok()
        {
            return format!("{label} {number}").into();
        }
    }

    if NAMED.contains(&name.as_ref()) {
        let mut chars = name.chars();
        if let Some(first) = chars.next() {
            return format!("{}{}", first.to_ascii_uppercase(), chars.as_str()).into();
        }
    }

    name.clone()
}

/// A port's dot on the block's edge, centered at `x`, `y` within the block.
fn port_dot(x: f32, y: f32, connected: bool, target: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let size = if target { PORT * 1.6 } else { PORT };

    div()
        .absolute()
        .left(units(x - size / 2.))
        .top(units(y - size / 2.))
        .size(units(size))
        .rounded_full()
        .border_2()
        .border_color(if target {
            theme.ring
        } else {
            theme.muted_foreground
        })
        .bg(if connected || target {
            theme.muted_foreground
        } else {
            theme.popover
        })
}

fn display_element(display: &preview::Display, cx: &App) -> AnyElement {
    let theme = cx.theme();

    match display {
        preview::Display::Text(text) => div()
            .font_family(theme.mono_font_family.clone())
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        preview::Display::Table {
            columns,
            rows,
            more,
        } => v_flex()
            .font_family(theme.mono_font_family.clone())
            .child(
                h_flex()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_color(theme.muted_foreground)
                    .children(columns.iter().map(|column| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .pr(units(6.))
                            .text_ellipsis()
                            .child(column.clone())
                    })),
            )
            .children(rows.iter().map(|row| {
                h_flex().children(row.iter().map(|cell| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .pr(units(6.))
                        .text_ellipsis()
                        .child(cell.clone())
                }))
            }))
            .when(*more > 0, |this| {
                this.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("{more} more rows")),
                )
            })
            .into_any_element(),
    }
}
