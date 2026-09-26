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
use crate::variables::{VariableScope, VariableStore};

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

    fn origin(&self, cx: &App) -> Option<Point<Pixels>> {
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
        Some(cursor.origin + scroll + point(px(0.), line_height + px(4.)))
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
    store: Entity<VariableStore>,
    snapshot: Option<(SharedString, usize)>,
    range: Option<Range<usize>>,
    suggestions: Vec<Suggestion>,
    selected: usize,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl VariableInput {
    pub fn new(
        target: VariableTarget,
        scope: Entity<VariableScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = VariableStore::global(cx);
        let input_subscription = match &target {
            VariableTarget::Input(input) => cx.observe_in(input, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
            VariableTarget::Editor(input) => cx.observe_in(input, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
        };
        let store_subscription = cx.observe_in(&store, window, |this, _, window, cx| {
            this.snapshot = None;
            this.refresh(window, cx);
        });
        let scope_subscription = cx.observe_in(&scope, window, |this, _, window, cx| {
            this.snapshot = None;
            this.refresh(window, cx);
        });

        Self {
            target,
            scope,
            store,
            snapshot: None,
            range: None,
            suggestions: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            _subscriptions: vec![input_subscription, store_subscription, scope_subscription],
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.target.snapshot(window, cx);
        if snapshot == self.snapshot {
            return;
        }
        self.snapshot = snapshot;
        self.range = None;
        self.suggestions.clear();
        self.selected = 0;
        self.scroll.set_offset(point(px(0.), px(0.)));

        if let Some((text, cursor)) = &self.snapshot
            && let Some((range, query)) = active_token(text, *cursor)
        {
            self.range = Some(range);
            let query = query.to_lowercase();
            let store = self.store.read(cx);
            let path = &self.scope.read(cx).path;
            if let Some(values) = store.environments.get(path) {
                self.suggestions.extend(
                    values
                        .keys()
                        .filter(|name| environment::valid_variable_name(name))
                        .map(|name| Suggestion {
                            name: name.clone(),
                            source: "Environment",
                        }),
                );
            }
            self.suggestions
                .extend(store.secrets.keys().map(|name| Suggestion {
                    name: format!("vault:{name}"),
                    source: "Secret",
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
                self.scroll.scroll_to_item(self.selected);
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

impl Render for VariableInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.range.is_none() || self.target.snapshot(window, cx).is_none() {
            return Empty.into_any_element();
        }
        let Some(origin) = self.target.origin(cx) else {
            return Empty.into_any_element();
        };
        let width = rems(24.)
            .to_pixels(window.rem_size())
            .min(window.bounds().size.width - px(16.));

        deferred(
            anchored()
                .position(origin)
                .snap_to_window_with_margin(px(8.))
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
                                .max_h(rems(16.))
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .when(self.suggestions.is_empty(), |list| {
                                    list.child(
                                        div()
                                            .p_2()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("No matching variables"),
                                    )
                                })
                                .children(self.suggestions.iter().enumerate().map(
                                    |(index, item)| {
                                        h_flex()
                                            .id(("variable", index))
                                            .role(Role::ListBoxOption)
                                            .aria_label(format!("{}, {}", item.name, item.source))
                                            .aria_selected(index == self.selected)
                                            .aria_position_in_set(index + 1)
                                            .aria_size_of_set(self.suggestions.len())
                                            .debug_selector(move || {
                                                format!("variable-suggestion-{index}")
                                            })
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
                                                        "Secret" => "S",
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
                                    },
                                )),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("↑ ↓ to navigate · Enter / Tab to insert · Esc to dismiss"),
                        ),
                ),
        )
        .into_any_element()
    }
}
