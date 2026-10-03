use flow::{Block, BlockKind, BlockType};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, spinner::Spinner, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::{method_color, method_label};
use serde_json::Value;

use super::{
    FlowEditor,
    editor::Layout,
    geometry::{BASE_REM, DETAIL_ZOOM, HEADER, PORT, ROW, body_height, port_offset},
    preview,
    run::BlockStatus,
};

/// Zoomed-out blocks show their titles only when they are at least this
/// wide on screen, in interface pixels.
const OUTLINE_TITLE_WIDTH: f32 = 36.;

/// Canvas pixels as rems, which the zoomed canvas scales.
fn units(value: f32) -> Rems {
    rems(value / BASE_REM)
}

impl FlowEditor {
    /// Interface pixels, which keep their size on screen at every zoom but
    /// follow the interface font size.
    fn screen(&self, value: f32) -> Pixels {
        self.rem * (value / BASE_REM)
    }
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
        if matches!(block.kind, BlockKind::Note { .. }) {
            return self.note_element(block, layout, cx);
        }
        if self.viewport.zoom < DETAIL_ZOOM {
            return self.block_outline(block, layout, cx);
        }

        let theme = cx.theme();
        let selected = self.selection.contains(&block.id);
        let status = self.run.blocks.get(&block.id);
        let origin = self.viewport.to_view(layout.bounds.origin, self.rem);
        let id = block.id.clone();
        let rows = layout.inputs.len().max(layout.outputs.len());
        let connecting = self.connecting_port();

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
            .bg(theme.popover)
            .shadow_sm()
            .text_color(theme.popover_foreground)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.press(event, Some(id.clone()), window, cx);
                }),
            )
            .child(self.block_header(block, layout, status, cx))
            .children((0..rows).map(|row| {
                h_flex()
                    .flex_none()
                    .h(units(ROW))
                    .px(units(12.))
                    .gap(units(8.))
                    .text_xs()
                    // A row without an input leaves its output the whole width.
                    .when_some(layout.inputs.get(row), |this, name| {
                        this.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(theme.muted_foreground)
                                .child(self.input_label(block, layout, name, cx)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_right()
                            .children(layout.outputs.get(row).map(|name| {
                                let sent = status.is_some_and(|status| status.sent(name));
                                let label = output_label(block, name);
                                div()
                                    .when(label.1, |this| {
                                        this.font_family(theme.mono_font_family.clone())
                                    })
                                    .when(!sent, |this| this.text_color(theme.muted_foreground))
                                    .when(sent, |this| this.font_weight(FontWeight::MEDIUM))
                                    // A request or a check that failed sent from Fail.
                                    .when(sent && name.as_ref() == "fail", |this| {
                                        this.text_color(theme.danger)
                                    })
                                    .child(label.0)
                            })),
                    )
            }))
            .children(self.block_body(block, layout, status, cx))
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

    /// An input's name. Variables the request's collection or the active
    /// environment fill are dimmed while nothing is connected to them.
    fn input_label(
        &self,
        block: &Block,
        layout: &Layout,
        name: &SharedString,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let filled = layout
            .request
            .as_ref()
            .is_some_and(|request| request.defined.contains(name.as_ref()))
            && self.flow.connection_into(&block.id, name).is_none();

        if !filled {
            return div()
                .text_ellipsis()
                .child(port_label(name))
                .into_any_element();
        }

        h_flex()
            .gap(units(4.))
            .min_w_0()
            .child(
                div()
                    .min_w_0()
                    .text_ellipsis()
                    .opacity(0.6)
                    .child(port_label(name)),
            )
            .child(
                div()
                    .flex_none()
                    .px(units(4.))
                    .rounded(theme.radius_tokens().sm)
                    .border_1()
                    .border_color(theme.border)
                    .opacity(0.8)
                    .child("env"),
            )
            .into_any_element()
    }

    /// A block without its text, for when the canvas is zoomed far out. It
    /// keeps its title while there is room for it.
    fn block_outline(&self, block: &Block, layout: &Layout, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let block_type = block.kind.block_type();
        let selected = self.selection.contains(&block.id);
        let status = self.run.blocks.get(&block.id);
        let origin = self.viewport.to_view(layout.bounds.origin, self.rem);
        let scale = self.viewport.scale(self.rem);
        let id = block.id.clone();
        let state = status.map(|status| {
            if status.running {
                theme.info
            } else if status.troubled() {
                theme.danger
            } else {
                theme.success
            }
        });
        let titled =
            layout.bounds.size.width * scale >= f32::from(self.screen(OUTLINE_TITLE_WIDTH));

        div()
            .id(SharedString::from(format!("flow-block-{}", block.id)))
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(units(layout.bounds.size.width))
            .h(units(layout.bounds.size.height))
            .overflow_hidden()
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
                    .bg(color(block_type, cx).opacity(0.3))
                    .children(state.map(|state| div().size(units(12.)).rounded_full().bg(state))),
            )
            // Unlike the rest of the block, the title keeps its size on
            // screen, so it stays readable.
            .when(titled, |this| {
                this.child(
                    div()
                        .px(self.screen(3.))
                        .pt(self.screen(1.))
                        .text_size(self.screen(10.))
                        .line_height(self.screen(12.))
                        .text_ellipsis()
                        .text_color(theme.foreground)
                        .child(self.block_title(block, cx)),
                )
            })
            .into_any_element()
    }

    /// A Note: text in a frame, behind the blocks it frames. Its corner
    /// resizes it.
    fn note_element(&self, block: &Block, layout: &Layout, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let selected = self.selection.contains(&block.id);
        let origin = self.viewport.to_view(layout.bounds.origin, self.rem);
        let id = block.id.clone();
        let resized = block.id.clone();
        let text = match &block.kind {
            BlockKind::Note { text, .. } => text.as_str(),
            _ => "",
        };
        let (heading, rest) = text.split_once('\n').unwrap_or((text, ""));
        let heading = SharedString::from(heading.trim().to_owned());
        let detail = self.viewport.zoom >= DETAIL_ZOOM;

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
            .when(detail, |this| this.overflow_hidden())
            .rounded(theme.radius_tokens().lg)
            .border_1()
            .border_color(if selected {
                theme.ring
            } else {
                theme.warning.opacity(0.35)
            })
            .bg(theme.warning.opacity(0.07))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.press(event, Some(id.clone()), window, cx);
                }),
            )
            .child(if detail {
                v_flex()
                    .p(units(12.))
                    .gap(units(4.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .when(heading.is_empty(), |this| {
                                this.font_weight(FontWeight::NORMAL)
                                    .text_color(theme.muted_foreground)
                            })
                            .child(if heading.is_empty() {
                                SharedString::from("Note")
                            } else {
                                heading
                            }),
                    )
                    .when(!rest.trim().is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(rest.trim().to_owned())),
                        )
                    })
                    .into_any_element()
            } else {
                // Zoomed out, a Note names its section above it at a
                // readable size, where the blocks it frames leave it be.
                div()
                    .absolute()
                    .left_0()
                    .top(-self.screen(18.))
                    .w_full()
                    .h(self.screen(16.))
                    .text_size(self.screen(13.))
                    .line_height(self.screen(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .text_ellipsis()
                    .child(heading)
                    .into_any_element()
            })
            .child(
                div()
                    .id(SharedString::from(format!("flow-note-resize-{}", block.id)))
                    .debug_selector({
                        let id = block.id.clone();
                        move || format!("flow-note-resize-{id}")
                    })
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .size(units(16.))
                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                    .child(
                        div()
                            .absolute()
                            .right(units(4.))
                            .bottom(units(4.))
                            .size(units(6.))
                            .border_r_2()
                            .border_b_2()
                            .border_color(theme.warning.opacity(0.6)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.start_resize(&resized, event, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn block_header(
        &self,
        block: &Block,
        layout: &Layout,
        status: Option<&BlockStatus>,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let block_type = block.kind.block_type();
        let custom = block
            .title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty());

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
            .when_some(layout.request.as_ref(), |this, request| {
                this.child(div().flex_none().child(method_label(request.method, cx)))
            })
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .child(self.block_title(block, cx)),
            )
            // A block with its own title still says what it is.
            .when(custom && block_type != BlockType::HttpRequest, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(block_type.name()),
                )
            })
            .child(div().flex_1())
            .children(status.and_then(|status| status_badge(block, status, cx)))
    }

    /// What a block shows below its ports: a summary of its settings, a
    /// Display block's data, or a message of its last run.
    fn block_body(
        &self,
        block: &Block,
        layout: &Layout,
        status: Option<&BlockStatus>,
        cx: &App,
    ) -> Option<AnyElement> {
        let theme = cx.theme();
        let last = status.and_then(|status| status.last.as_ref());
        let body = div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .px(units(12.))
            .py(units(6.))
            .text_xs()
            .text_color(theme.muted_foreground);

        if let Some(error) = last.and_then(|run| run.error.clone()) {
            return Some(
                body.text_color(theme.danger)
                    .line_clamp(2)
                    .child(SharedString::from(error))
                    .into_any_element(),
            );
        }

        // Settings shown on two lines where the block has room for them.
        let lines = if body_height(&block.kind) >= 56. {
            2
        } else {
            1
        };
        let code = |text: &str| {
            div()
                .font_family(theme.mono_font_family.clone())
                .line_clamp(lines)
                .text_ellipsis()
                .child(SharedString::from(text.to_owned()))
                .into_any_element()
        };
        let text = |text: String| div().text_ellipsis().child(text).into_any_element();

        let content = match &block.kind {
            BlockKind::Start { input } => Some(if input.trim().is_empty() {
                text("Sends the run's input".to_owned())
            } else {
                code(input)
            }),
            BlockKind::HttpRequest { request } => Some(match &layout.request {
                // The header names the request, unless the block has its own title.
                Some(info) if block.title.as_deref().is_some_and(|t| !t.trim().is_empty()) => div()
                    .text_ellipsis()
                    .text_color(theme.foreground)
                    .child(info.name.clone())
                    .into_any_element(),
                Some(info) => code(&info.path),
                None if request.is_empty() => div()
                    .text_color(theme.warning)
                    .child("Choose a request")
                    .into_any_element(),
                None => div()
                    .text_color(theme.danger)
                    .child("The request is no longer saved")
                    .into_any_element(),
            }),
            BlockKind::Evaluate { expression, .. } => Some(code(expression)),
            BlockKind::If { condition, .. } => Some(code(condition)),
            BlockKind::Validate { schema } => Some(match schema_summary(schema) {
                Ok(summary) => text(summary),
                Err(error) => div()
                    .text_ellipsis()
                    .text_color(theme.danger)
                    .child(error)
                    .into_any_element(),
            }),
            BlockKind::Delay { milliseconds } => Some(text(format!("Waits {milliseconds} ms"))),
            BlockKind::Display { .. } => {
                return Some(
                    body.text_color(theme.foreground)
                        .child(match status.and_then(|status| status.display.as_ref()) {
                            Some((_, display)) => display_element(display, cx),
                            None => div()
                                .text_color(theme.muted_foreground)
                                .child("Run the flow to see its data")
                                .into_any_element(),
                        })
                        .into_any_element(),
                );
            }
            BlockKind::String { value } => Some(code(&format!("\"{value}\""))),
            BlockKind::Number { value } => Some(code(&value.to_string())),
            BlockKind::Boolean { value } => Some(code(&value.to_string())),
            BlockKind::Date { value } => Some(code(value)),
            BlockKind::Select { path } => Some(if path.trim().is_empty() {
                text("All of the data".to_owned())
            } else {
                code(path)
            }),
            BlockKind::Record { fields } => Some(code(&format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|field| field.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
            BlockKind::List { items } => Some(text(format!(
                "{} item{}",
                items.len(),
                if items.len() == 1 { "" } else { "s" }
            ))),
            BlockKind::Template { template, .. } => Some(code(template)),
            BlockKind::SetVariable { name } | BlockKind::GetVariable { name } => Some(code(name)),
            // Their ports say all there is to know.
            BlockKind::Or
            | BlockKind::Repeat
            | BlockKind::For
            | BlockKind::Collect
            | BlockKind::Log
            | BlockKind::Null
            | BlockKind::Now
            | BlockKind::Output { .. }
            | BlockKind::Condition { .. }
            | BlockKind::Note { .. } => None,
        };
        let notice = last.and_then(|run| run.notice.clone());

        if content.is_none() && notice.is_none() {
            return None;
        }

        Some(
            body.children(content)
                .when_some(notice, |this, notice| {
                    this.child(
                        div()
                            .text_color(theme.warning)
                            .text_ellipsis()
                            .child(SharedString::from(notice)),
                    )
                })
                .into_any_element(),
        )
    }
}

/// How the last run went, in the header: the HTTP status a request
/// received, or whether the block failed, and how often it ran.
fn status_badge(block: &Block, status: &BlockStatus, cx: &App) -> Option<AnyElement> {
    let theme = cx.theme();

    if status.running {
        return Some(Spinner::new().xsmall().color(theme.info).into_any_element());
    }
    if status.runs == 0 {
        return None;
    }

    let troubled = status.troubled();
    let mark = match (&block.kind, status.http_status()) {
        (BlockKind::HttpRequest { .. }, http) => {
            let color = if troubled {
                theme.danger
            } else {
                theme.success
            };
            div()
                .flex_none()
                .px(units(6.))
                .rounded_full()
                .bg(color.opacity(0.14))
                .text_color(color)
                .font_weight(FontWeight::SEMIBOLD)
                .child(match http {
                    Some(code) => code.to_string(),
                    None => "Error".to_owned(),
                })
                .into_any_element()
        }
        _ if troubled => Icon::default()
            .path("icons/circle-alert.svg")
            .size(units(14.))
            .text_color(theme.danger)
            .into_any_element(),
        _ => Icon::new(IconName::Check)
            .size(units(14.))
            .text_color(theme.success)
            .into_any_element(),
    };

    Some(
        h_flex()
            .flex_none()
            .gap(units(4.))
            .text_xs()
            .text_color(theme.muted_foreground)
            .when(status.runs > 1, |this| {
                this.child(format!("×{}", status.runs))
            })
            .child(mark)
            .into_any_element(),
    )
}

/// An output's label, and whether it is code. A Condition block's outputs
/// show the conditions they stand for.
fn output_label(block: &Block, name: &SharedString) -> (SharedString, bool) {
    if let BlockKind::Condition { conditions, .. } = &block.kind
        && let Some(index) = name
            .strip_prefix("condition")
            .and_then(|number| number.parse::<usize>().ok())
        && let Some(condition) = conditions
            .get(index.wrapping_sub(1))
            .map(|condition| condition.trim())
            .filter(|condition| !condition.is_empty())
    {
        return (SharedString::from(condition.replace('\n', " ")), true);
    }

    (port_label(name), false)
}

/// What a JSON Schema checks, in a few words, such as `object · requires
/// body, id`, or why it is not a schema.
pub(super) fn schema_summary(schema: &str) -> Result<String, String> {
    let schema: Value =
        serde_json::from_str(schema).map_err(|_| "The schema is not JSON".to_owned())?;
    let Value::Object(schema) = schema else {
        return match schema {
            Value::Bool(true) => Ok("Accepts anything".to_owned()),
            Value::Bool(false) => Ok("Accepts nothing".to_owned()),
            _ => Err("A schema is an object".to_owned()),
        };
    };

    let names = |value: Option<&Value>| -> Vec<String> {
        match value {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
            Some(Value::String(name)) => vec![name.clone()],
            _ => Vec::new(),
        }
    };
    let mut parts = Vec::new();
    let types = names(schema.get("type"));
    if !types.is_empty() {
        parts.push(types.join(" or "));
    }
    let required = names(schema.get("required"));
    if !required.is_empty() {
        parts.push(format!("requires {}", required.join(", ")));
    } else if let Some(Value::Object(properties)) = schema.get("properties") {
        parts.push(format!(
            "{} propert{}",
            properties.len(),
            if properties.len() == 1 { "y" } else { "ies" }
        ));
    }

    Ok(if parts.is_empty() {
        "Checks a JSON Schema".to_owned()
    } else {
        parts.join(" · ")
    })
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
