use std::ops::Range;

use environment::GENERATED_VARIABLES;
use gpui_kit::{
    component::{
        input::{EditorState, InputState},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};

use super::token::active_token;
use crate::variables::VariableScope;

#[derive(Clone)]
pub(crate) enum VariableTarget {
    Input(Entity<InputState>),
    Editor(Entity<EditorState>),
}

impl VariableTarget {
    fn snapshot(&self, window: &Window, cx: &App) -> Option<(SharedString, usize)> {
        match self {
            Self::Input(input) => {
                let input = input.read(cx);
                (input.focus_handle(cx).is_focused(window) && input.selected_range().is_empty())
                    .then(|| (input.value(), input.cursor()))
            }
            Self::Editor(input) => {
                let input = input.read(cx);
                (input.focus_handle(cx).is_focused(window) && input.selected_range().is_empty())
                    .then(|| (input.value(), input.cursor()))
            }
        }
    }

    fn origin(&self, window: &Window, cx: &App) -> Option<Point<Pixels>> {
        let (cursor, line_height, scroll) = match self {
            Self::Input(input) => {
                let input = input.read(cx);
                let (cursor, height) = input.cursor_layout()?;
                (cursor, height, input.scroll_offset())
            }
            Self::Editor(input) => {
                let input = input.read(cx);
                let (cursor, height) = input.cursor_layout()?;
                (cursor, height, input.scroll_offset())
            }
        };
        Some(
            cursor.origin
                + point(px(0.), scroll.y)
                + point(
                    px(0.),
                    line_height + rems(0.25).to_pixels(window.rem_size()),
                ),
        )
    }

    fn replace(&self, range: Range<usize>, text: String, window: &mut Window, cx: &mut App) {
        match self {
            Self::Input(input) => input.update(cx, |input, cx| {
                let value = input.value();
                let utf16 = value[..range.start].encode_utf16().count()
                    ..value[..range.end].encode_utf16().count();
                let selection = input.selected_range();
                input.set_selected_range(selection, cx);
                input.replace_text_in_range(Some(utf16), &text, window, cx);
                input.focus(window, cx);
            }),
            Self::Editor(input) => input.update(cx, |input, cx| {
                let value = input.value();
                let utf16 = value[..range.start].encode_utf16().count()
                    ..value[..range.end].encode_utf16().count();
                let selection = input.selected_range();
                input.set_selected_range(selection, cx);
                input.replace_text_in_range(Some(utf16), &text, window, cx);
                input.focus(window, cx);
            }),
        }
    }
}

struct Suggestion {
    name: String,
    source: &'static str,
}

pub(crate) struct VariableInput {
    target: VariableTarget,
    scope: Entity<VariableScope>,
    environment_names: Vec<String>,
    snapshot: Option<(SharedString, usize)>,
    range: Option<Range<usize>>,
    suggestions: Vec<Suggestion>,
    selected: usize,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl VariableInput {
    pub fn new(
        target: VariableTarget,
        scope: Entity<VariableScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input_subscription = match &target {
            VariableTarget::Input(input) => cx.observe_in(input, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
            VariableTarget::Editor(input) => cx.observe_in(input, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
        };
        let scope_subscription = cx.observe_in(&scope, window, |this, _, window, cx| {
            this.snapshot = None;
            this.range = None;
            this.refresh(window, cx);
        });

        Self {
            target,
            scope,
            environment_names: Vec::new(),
            snapshot: None,
            range: None,
            suggestions: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![input_subscription, scope_subscription],
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.target.snapshot(window, cx);
        if snapshot == self.snapshot {
            return;
        }
        self.snapshot = snapshot;
        let was_open = self.range.take().is_some();
        self.suggestions.clear();
        self.selected = 0;
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);

        if let Some((text, cursor)) = &self.snapshot
            && let Some((range, query)) = active_token(text, *cursor)
        {
            self.range = Some(range);
            let query = query.to_lowercase();
            if !was_open {
                let scope = self.scope.read(cx);
                self.environment_names = scope
                    .values(cx)
                    .unwrap_or_else(|_| {
                        scope.session.values(environment::VariableValues::default())
                    })
                    .environment
                    .into_keys()
                    .filter(|name| environment::valid_variable_name(name))
                    .collect();
            }
            self.suggestions
                .extend(self.environment_names.iter().map(|name| Suggestion {
                    name: name.clone(),
                    source: "Environment",
                }));
            self.suggestions.sort_by(|a, b| a.name.cmp(&b.name));
            self.suggestions
                .extend(GENERATED_VARIABLES.iter().map(|(name, _)| Suggestion {
                    name: (*name).into(),
                    source: "Generated",
                }));
            self.suggestions
                .retain(|item| item.name.to_lowercase().contains(&query));
        }

        cx.notify();
    }

    fn render_suggestion(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.suggestions[index];

        h_flex()
            .id(item.name.clone())
            .role(Role::ListBoxOption)
            .aria_label(format!("{}, {}", item.name, item.source))
            .aria_selected(index == self.selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.suggestions.len())
            .debug_selector(move || format!("variable-suggestion-{index}"))
            .w_full()
            .h_8()
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius_tokens().md)
            .when(index == self.selected, |row| {
                row.bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
            })
            .hover(|row| row.bg(cx.theme().muted))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(match item.source {
                        "Environment" => "E",
                        _ => "G",
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .child(item.name.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(item.source),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.accept(index, window, cx);
                }),
            )
            .into_any_element()
    }

    fn accept(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.suggestions.get(index) else {
            return;
        };
        let Some(range) = self.range.take() else {
            return;
        };
        let text = format!("{{{{{}}}}}", item.name);
        self.target.replace(range, text, window, cx);
        self.suggestions.clear();
        cx.notify();
    }

    fn command(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.range.is_none() || self.target.snapshot(window, cx).is_none() {
            return;
        }

        match key {
            "escape" => {
                self.range = None;
            }
            "enter" | "tab" if !self.suggestions.is_empty() => {
                self.accept(self.selected, window, cx)
            }
            "up" | "down" if !self.suggestions.is_empty() => {
                let count = self.suggestions.len();
                self.selected = if key == "down" {
                    (self.selected + 1) % count
                } else {
                    (self.selected + count - 1) % count
                };
                self.scroll
                    .scroll_to_item(self.selected, ScrollStrategy::Nearest);
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }
}

pub(crate) fn with_variables(completion: &Entity<VariableInput>, content: impl IntoElement) -> Div {
    use gpui_kit::component::input::{Enter, Escape, IndentInline, MoveDown, MoveUp};

    let enter = completion.clone();
    let tab = completion.clone();
    let up = completion.clone();
    let down = completion.clone();
    let escape = completion.clone();
    div()
        .relative()
        .min_w_0()
        .w_full()
        .capture_action(move |action: &Enter, window, cx| {
            if !action.secondary && !action.shift {
                enter.update(cx, |this, cx| this.command("enter", window, cx));
            }
        })
        .capture_action(move |_: &IndentInline, window, cx| {
            tab.update(cx, |this, cx| this.command("tab", window, cx))
        })
        .capture_action(move |_: &MoveUp, window, cx| {
            up.update(cx, |this, cx| this.command("up", window, cx))
        })
        .capture_action(move |_: &MoveDown, window, cx| {
            down.update(cx, |this, cx| this.command("down", window, cx))
        })
        .capture_action(move |_: &Escape, window, cx| {
            escape.update(cx, |this, cx| this.command("escape", window, cx))
        })
        .child(content)
        .child(completion.clone())
}

impl VariableInput {
    fn render_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.range.is_none() || self.target.snapshot(window, cx).is_none() {
            return Empty.into_any_element();
        }
        let Some(origin) = self.target.origin(window, cx) else {
            return Empty.into_any_element();
        };
        let margin = rems(0.5).to_pixels(window.rem_size());
        let width = rems(18.)
            .to_pixels(window.rem_size())
            .min((window.bounds().size.width - margin * 2.).max(px(0.)));

        anchored()
            .position(origin)
            .snap_to_window_with_margin(margin)
            .child(
                v_flex()
                    .id("variable-completions")
                    .debug_selector(|| "variable-completions".into())
                    .w(width)
                    .p_1()
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_tokens().lg)
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.range = None;
                        cx.notify();
                    }))
                    .child(
                        v_flex()
                            .id("variable-suggestions")
                            .role(Role::ListBox)
                            .aria_label("Variable suggestions")
                            .child(if self.suggestions.is_empty() {
                                div()
                                    .p_2()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No matching variables")
                                    .into_any_element()
                            } else {
                                uniform_list(
                                    "variable-list",
                                    self.suggestions.len(),
                                    cx.processor(|this, range: Range<usize>, _, cx| {
                                        range
                                            .map(|index| this.render_suggestion(index, cx))
                                            .collect()
                                    }),
                                )
                                .w_full()
                                .h(rems(2. * self.suggestions.len().min(8) as f32))
                                .track_scroll(&self.scroll)
                                .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("↑↓ Navigate · Enter Insert · Esc Close"),
                    ),
            )
            .into_any_element()
    }
}

impl Render for VariableInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.range.is_none() {
            return Empty.into_any_element();
        }

        let completion = cx.entity();

        // Defer positioning until the input has laid out this frame's caret.
        deferred(
            canvas(
                move |_, window, cx| {
                    let mut popover = completion
                        .update(cx, |completion, cx| completion.render_popover(window, cx));
                    popover.prepaint_as_root(
                        Point::default(),
                        window.viewport_size().map(AvailableSpace::Definite),
                        window,
                        cx,
                    );
                    popover
                },
                |_, mut popover, window, cx| popover.paint(window, cx),
            )
            .absolute(),
        )
        .into_any_element()
    }
}
