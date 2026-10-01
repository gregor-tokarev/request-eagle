use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use environment::GENERATED_VARIABLES;
use gpui_kit::{
    component::{
        input::{EditorState, InputState},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use ropey::{Rope, extra::esoterica::ropes_are_instances};

use crate::variables::VariableScope;

#[derive(Clone)]
pub(crate) enum VariableTarget {
    Input(Entity<InputState>),
    Editor(Entity<EditorState>),
}

impl VariableTarget {
    fn text(&self, cx: &App) -> Rope {
        match self {
            Self::Input(input) => input.read(cx).text().clone(),
            Self::Editor(input) => input.read(cx).text().clone(),
        }
    }

    /// Where a range of a single-line input is on screen this frame. Unlike
    /// a chip's, its end cannot wrap onto another row.
    fn range_bounds(&self, range: &Range<usize>, cx: &App) -> Option<Bounds<Pixels>> {
        let Self::Input(input) = self else {
            return None;
        };

        input
            .read(cx)
            .range_to_bounds(range)
            .filter(|bounds| bounds.size.width > px(0.))
    }

    /// The visible text area and where each chip is on screen this frame.
    fn chip_bounds(
        &self,
        chips: &[Range<usize>],
        cx: &App,
    ) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>) {
        match self {
            Self::Input(input) => {
                let input = input.read(cx);
                let bounds = chip_bounds(chips, input.line_height(), |range| {
                    input.range_to_bounds(range)
                });
                (input.input_bounds(), bounds)
            }
            Self::Editor(input) => {
                let input = input.read(cx);
                let bounds = chip_bounds(chips, input.line_height(), |range| {
                    input.range_to_bounds(range)
                });
                (input.input_bounds(), bounds)
            }
        }
    }

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
    /// Byte ranges of the `{{variable}}` references that resolve.
    chips: Vec<Range<usize>>,
    /// Byte ranges of references that sending could not resolve.
    unresolved: Vec<Range<usize>>,
    /// In a request URL, the `:name` path variables that have a value.
    path_variables: Option<HashSet<String>>,
    /// Byte ranges of the URL's path variables, and whether each has a value.
    paths: Vec<(Range<usize>, bool)>,
    /// The text and names the chips were found with.
    chipped: (Rope, Rc<HashSet<String>>),
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
            // Repaint to recolor the chips, even when completion is closed.
            cx.notify();
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
            chips: Vec::new(),
            unresolved: Vec::new(),
            path_variables: None,
            paths: Vec::new(),
            chipped: Default::default(),
            _subscriptions: vec![input_subscription, scope_subscription],
        }
    }

    /// Also mark the `:name` path variables of a request URL, colored by
    /// whether `filled` has a value for them.
    pub fn with_path_variables(mut self, filled: HashSet<String>) -> Self {
        self.path_variables = Some(filled);
        self
    }

    pub fn set_path_variables(&mut self, filled: HashSet<String>, cx: &mut Context<Self>) {
        self.path_variables = Some(filled);
        // Find the chips again on the next paint.
        self.chipped = Default::default();
        cx.notify();
    }

    /// Find and color the chips when the text or the resolvable names changed.
    /// This runs as the field paints, so hidden tabs never read environments.
    fn update_chips(&mut self, cx: &mut App) {
        let text = self.target.text(cx);
        let names = self.scope.update(cx, |scope, cx| scope.names(cx));

        if ropes_are_instances(&self.chipped.0, &text) && Rc::ptr_eq(&self.chipped.1, &names) {
            return;
        }

        let source = text.to_string();
        (self.chips, self.unresolved) = variable_references(&source).partition(|chip| {
            let name = source[chip.start + 2..chip.end - 2].trim();

            // Sending resolves `$` names only as generated values.
            if name.starts_with('$') {
                environment::is_generated_variable(name)
            } else {
                names.contains(name)
            }
        });
        if let Some(filled) = &self.path_variables {
            self.paths = request::path_variables(&source)
                .map(|(range, name)| (range, filled.contains(name)))
                .collect();
        }
        self.chipped = (text, names);
    }

    /// Paint each `{{variable}}` as a rounded chip over its text. The text
    /// stays ordinary input text; the input only reports where it is.
    fn paint_chips(&mut self, window: &mut Window, cx: &mut App) {
        self.update_chips(cx);
        let outset = point(rems(0.125).to_pixels(window.rem_size()), -px(1.));
        let radius = cx.theme().radius_tokens().sm;
        let (info, danger) = (cx.theme().info, cx.theme().danger);

        let (visible, resolved) = self.target.chip_bounds(&self.chips, cx);
        let (_, unresolved) = self.target.chip_bounds(&self.unresolved, cx);
        // A path variable without a value is sent as written.
        let paths = self.paths.iter().filter_map(|(range, filled)| {
            let bounds = self.target.range_bounds(range, cx)?;
            Some((bounds, if *filled { info } else { danger }))
        });
        let chips: Vec<_> = resolved
            .into_iter()
            .map(|chip| (chip, info))
            .chain(unresolved.into_iter().map(|chip| (chip, danger)))
            .chain(paths)
            .collect();
        // Let a chip at either edge of the text keep its padding.
        let visible = visible.dilate(outset.x);

        window.with_content_mask(Some(ContentMask { bounds: visible }), |window| {
            for (chip, color) in chips {
                let chip = Bounds::from_corners(chip.origin - outset, chip.bottom_right() + outset);
                window.paint_quad(fill(chip, color.opacity(0.25)).corner_radii(radius));
            }
        });
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
                    .unwrap_or_else(|_| scope.session.values(HashMap::new(), HashMap::new()))
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
        let chips = cx.entity();
        let completion = chips.clone();

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // The input is an earlier sibling, so its layout is current by now.
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| {
                        chips.update(cx, |chips, cx| chips.paint_chips(window, cx))
                    },
                )
                .size_full(),
            )
            .when(self.range.is_some(), |this| {
                // Defer positioning until the input has laid out this frame's caret.
                this.child(deferred(
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
                ))
            })
    }
}

/// A token around the caret, including any existing suffix and closing braces.
/// Offsets use UTF-8 bytes, matching GPUI's selection API.
pub(crate) fn active_token(text: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let before = text.get(..cursor)?;
    let start = before.rfind("{{")?;
    let query = &before[start + 2..];

    if query
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '{' | '}' | '"' | '\''))
    {
        return None;
    }

    let suffix = &text[cursor..];
    let name_end = suffix
        .find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '$' | ':')))
        .unwrap_or(suffix.len());
    let mut end = cursor + name_end;
    if text[end..].starts_with("}}") {
        end += 2;
    } else if text[end..].starts_with('}') {
        end += 1;
    }

    Some((start..end, query))
}

/// Where each chip is on screen. Chips that are folded away, scrolled out of
/// view or wrapped onto another row are left out.
pub(crate) fn chip_bounds(
    chips: &[Range<usize>],
    line_height: Option<Pixels>,
    range_to_bounds: impl Fn(&Range<usize>) -> Option<Bounds<Pixels>>,
) -> Vec<Bounds<Pixels>> {
    let Some(line_height) = line_height else {
        return Vec::new();
    };

    chips
        .iter()
        .filter_map(|chip| {
            // A soft wrap right after a chip resolves its end to the next row,
            // so measure up to the last brace and add the brace before it.
            let inner = range_to_bounds(&(chip.start..chip.end - 1))?;
            let brace = range_to_bounds(&(chip.end - 2..chip.end - 1))?;
            let width = inner.size.width + brace.size.width;

            // Folded text collapses to zero width at the next visible row.
            (inner.size.height <= line_height && inner.size.width > px(0.))
                .then(|| Bounds::new(inner.origin, size(width, inner.size.height)))
        })
        .collect()
}

/// Byte ranges of `{{variable}}` references. `{{!literal}}` escapes are sent
/// as written. A reference ends at the first `}}` on its line; an unclosed
/// `{{` does not claim later lines. Each byte is scanned once.
pub(crate) fn variable_references(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut offset = 0;

    std::iter::from_fn(move || {
        loop {
            let start = offset + text[offset..].find("{{")?;
            let mut cursor = start + 2;

            let close = loop {
                cursor += text[cursor..].find(['}', '\n'])?;
                if text[cursor..].starts_with("}}") {
                    break Some(cursor);
                }
                if text[cursor..].starts_with('\n') {
                    break None;
                }
                cursor += 1;
            };

            // No reference on this line can close once it ends.
            let Some(close) = close else {
                offset = cursor + 1;
                continue;
            };

            offset = close + 2;
            if !text[start + 2..close].starts_with('!') {
                return Some(start..offset);
            }
        }
    })
}
