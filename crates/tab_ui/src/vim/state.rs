use gpui_kit::component::{
    ActiveTheme as _,
    input::{EditorState, Escape, InputEvent, Redo, RopeExt, Undo},
};
use gpui_kit::{prelude::*, *};
use std::ops::Range;

use super::motions::{first_nonblank, line, lines, motion, newline, next, normal_cursor, previous};
use super::{column::Column, insertion::Insertion};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
    Visual {
        anchor: usize,
        cursor: usize,
        linewise: bool,
    },
}

/// One editor's modal state. The interceptor runs before GPUI's input bindings,
/// but only consumes keys while this exact editor has focus.
pub(crate) struct Vim {
    editor: Entity<EditorState>,
    enabled: bool,
    mode: Mode,
    count: usize,
    operator: Option<(char, usize)>,
    go: bool,
    selection: Range<usize>,
    desired_column: Option<Column>,
    insertion: Option<Insertion>,
    _subscriptions: Vec<Subscription>,
}

impl Vim {
    pub(super) fn normal_editor(&self) -> Option<&Entity<EditorState>> {
        (self.enabled && self.mode == Mode::Normal).then_some(&self.editor)
    }

    pub(crate) fn new(editor: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if this.enabled && this.editor.focus_handle(cx).is_focused(window) {
                    this.keystroke(&event.keystroke, window, cx);
                }
            });
        });
        let preferences = cx.observe_global::<preferences::Preferences>(|this, cx| {
            let enabled = cx.global::<preferences::Preferences>().vim_mode;

            if this.enabled != enabled {
                this.enabled = enabled;
                this.insertion = None;
                this.mode = Mode::Normal;
                this.reset_pending();
                this.desired_column = None;
                cx.notify();
            }
        });
        let changes = cx.subscribe(&editor, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur) {
                this.insertion = None;
                this.reset_pending();
                this.desired_column = None;
            }

            if matches!(event, InputEvent::Change) {
                this.reset_pending();
                this.desired_column = None;

                if matches!(this.mode, Mode::Visual { .. }) {
                    this.mode = Mode::Normal;
                    cx.notify();
                }
            }
        });
        let selection_changes = cx.observe(&editor, |this, editor, cx| {
            let selection = editor.read(cx).selected_range();

            if selection != this.selection {
                this.selection = selection;
                this.reset_pending();
                this.desired_column = None;

                if matches!(this.mode, Mode::Visual { .. }) {
                    this.mode = Mode::Normal;
                    cx.notify();
                }
            }
        });

        Self {
            selection: editor.read(cx).selected_range(),
            editor,
            enabled: cx
                .try_global::<preferences::Preferences>()
                .is_some_and(|p| p.vim_mode),
            mode: Mode::Normal,
            count: 0,
            operator: None,
            go: false,
            desired_column: None,
            insertion: None,
            _subscriptions: vec![keys, preferences, changes, selection_changes],
        }
    }

    fn reset_pending(&mut self) {
        self.count = 0;
        self.operator = None;
        self.go = false;
    }

    fn select(&mut self, range: Range<usize>, cx: &mut App) {
        self.editor
            .update(cx, |editor, cx| editor.set_selected_range(range, cx));
        self.selection = self.editor.read(cx).selected_range();
    }

    fn normal(&mut self, cursor: usize, cx: &mut Context<Self>) {
        self.mode = Mode::Normal;
        self.reset_pending();
        let cursor = normal_cursor(self.editor.read(cx).text(), cursor);
        self.select(cursor..cursor, cx);
        cx.notify();
    }

    fn insert(&mut self, cursor: usize, cx: &mut Context<Self>) {
        self.mode = Mode::Insert;
        self.reset_pending();
        self.desired_column = None;
        self.select(cursor..cursor, cx);
        cx.notify();
    }

    fn replace(&self, range: Range<usize>, text: &str, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(range, cx);
            editor.replace(text.to_owned(), window, cx);
        });
    }

    fn visual_range(&self, cx: &App) -> Option<Range<usize>> {
        let Mode::Visual {
            anchor,
            cursor,
            linewise,
        } = self.mode
        else {
            return None;
        };
        let text = self.editor.read(cx).text();
        let anchor = anchor.min(text.len());
        let cursor = cursor.min(text.len());

        Some(if linewise {
            lines(text, anchor, cursor)
        } else {
            anchor.min(cursor)..next(text, anchor.max(cursor))
        })
    }

    fn operate(
        &mut self,
        operator: char,
        mut range: Range<usize>,
        linewise: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.desired_column = None;
        let text = self.editor.read(cx).text();
        let mut copied = text.slice(range.clone()).to_string();

        if linewise && !copied.ends_with('\n') {
            copied.push_str(newline(text, range.start));
        }

        if !copied.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
                copied,
                serde_json::json!({ "request_eagle_vim_linewise": linewise }),
            ));
        }

        let mut cursor = if operator == 'y' && linewise && self.mode == Mode::Normal {
            self.editor.read(cx).cursor()
        } else {
            range.start
        };

        if operator != 'y' {
            let text = self.editor.read(cx).text();

            // Keep an empty line for cc; dd of the final line also removes its
            // preceding newline so it does not leave an extra blank line.
            let ends_in_newline =
                range.end > range.start && text.char_at(range.end - 1) == Some('\n');
            let replacement = if linewise && operator == 'c' && ends_in_newline {
                newline(text, range.end - 1)
            } else {
                ""
            };

            if linewise
                && operator == 'd'
                && !ends_in_newline
                && range.end == text.len()
                && range.start > 0
            {
                range.start = previous(text, range.start);

                cursor = range.start;
            }

            self.replace(range, replacement, window, cx);

            if linewise && operator == 'd' {
                cursor = first_nonblank(
                    self.editor.read(cx).text(),
                    cursor.min(self.editor.read(cx).text().len()),
                );
            }
        }

        if operator == 'c' {
            self.insert(cursor, cx);
        } else {
            self.normal(cursor, cx);
        }
    }

    fn paste(&mut self, before: bool, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(mut value) = item.text().filter(|value| !value.is_empty()) else {
            return;
        };
        let linewise = item.entries().first().is_some_and(|entry| {
            matches!(entry, ClipboardEntry::String(text) if text.metadata_json::<serde_json::Value>()
                .is_some_and(|metadata| metadata["request_eagle_vim_linewise"] == true))
        });
        let editor = self.editor.read(cx);
        let text = editor.text();
        let cursor = editor.cursor();

        if linewise {
            value = value.replace("\r\n", "\n");

            if newline(text, cursor) == "\r\n" {
                value = value.replace('\n', "\r\n");
            }
        }

        value = value.repeat(count);
        let visual = self.visual_range(cx);
        let linewise_selection = matches!(self.mode, Mode::Visual { linewise: true, .. });
        let mut position = cursor;
        let mut separator_len = 0;

        if visual.is_none() {
            position = if linewise {
                if before {
                    line(text, cursor).start
                } else {
                    lines(text, cursor, cursor).end
                }
            } else if before {
                cursor
            } else {
                next(text, cursor).min(line(text, cursor).end)
            };

            if linewise && text.len() == 0 {
                let content = value.strip_suffix(newline(text, cursor)).unwrap_or(&value);
                value.truncate(content.len());
            } else if linewise
                && !before
                && position == text.len()
                && text.offset_to_point(cursor).row + 1 == text.lines_len()
            {
                let separator = newline(text, cursor);
                separator_len = separator.len();
                let pasted = value
                    .strip_suffix("\r\n")
                    .or_else(|| value.strip_suffix('\n'))
                    .unwrap_or(&value);
                value = format!("{separator}{pasted}");
            }
        }

        let range = visual.clone().unwrap_or(position..position);
        let start = range.start;

        if linewise_selection {
            if !linewise {
                value.push_str(newline(text, start));
            }

            if matches!(self.mode, Mode::Visual { anchor, cursor, .. }
                if text.offset_to_point(anchor.max(cursor)).row + 1 == text.lines_len())
            {
                let content = value
                    .strip_suffix("\r\n")
                    .or_else(|| value.strip_suffix('\n'))
                    .unwrap_or(&value);
                value.truncate(content.len());
            }
        } else if visual.is_some() && linewise {
            let separator = newline(text, start);
            separator_len = separator.len();
            value.insert_str(0, separator);
        }

        if visual.is_some() && !before {
            let mut replaced = text.slice(range.clone()).to_string();

            if linewise_selection && !replaced.ends_with('\n') {
                replaced.push_str(newline(text, range.start));
            }

            cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
                replaced,
                serde_json::json!({ "request_eagle_vim_linewise": linewise_selection }),
            ));
        }

        self.replace(range, &value, window, cx);
        let cursor = if linewise || linewise_selection {
            first_nonblank(self.editor.read(cx).text(), start + separator_len)
        } else {
            previous(self.editor.read(cx).text(), start + value.len())
        };
        self.normal(cursor, cx);
    }

    fn keystroke(&mut self, stroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = stroke.modifiers;
        let escape = stroke.key == "escape" || (modifiers.control && stroke.key == "[");

        if escape {
            if let Some(insertion) = self.insertion.take() {
                let editor = self.editor.read(cx);
                let cursor = editor.cursor();

                if let Some(repeated) = insertion.finish(editor.text(), cursor)
                    && !repeated.is_empty()
                {
                    self.replace(cursor..cursor, &repeated, window, cx);
                }
            }

            let editor = self.editor.read(cx);
            let mut cursor = editor.cursor();

            if self.mode == Mode::Insert && cursor > line(editor.text(), cursor).start {
                cursor = previous(editor.text(), cursor);
            } else if let Mode::Visual {
                cursor: visual_cursor,
                ..
            } = self.mode
            {
                cursor = visual_cursor;
            }

            self.normal(cursor, cx);
            window.dispatch_action(Box::new(Escape), cx);
        } else if self.mode == Mode::Insert {
            return;
        } else if modifiers.control && !modifiers.alt && !modifiers.platform && stroke.key == "r" {
            let count = if self.go { 0 } else { self.count.max(1) };
            self.reset_pending();

            for _ in 0..count {
                window.dispatch_action(Box::new(Redo), cx);
            }
        } else if modifiers.control
            || modifiers.alt
            || modifiers.platform
            || window.has_pending_keystrokes()
        {
            self.reset_pending();
            let mut input = window
                .pending_input_keystrokes()
                .unwrap_or_default()
                .to_vec();
            input.push(stroke.clone());
            let keymap = cx.key_bindings();
            let (bindings, _) = keymap
                .borrow()
                .bindings_for_input(&input, &window.context_stack());

            // Keep application shortcuts (including user remappings), while
            // preventing native editor actions from bypassing modal editing.
            if bindings.first().is_some_and(|binding| {
                let action = binding.action().name();
                !action.starts_with("input::") || matches!(action, "input::Copy" | "input::Search")
            }) || window
                .possible_bindings_for_input(&input)
                .iter()
                .any(|binding| !binding.action().name().starts_with("input::"))
            {
                return;
            }
        } else {
            let key = stroke.key_char.as_deref().unwrap_or(&stroke.key);
            let key = if modifiers.shift {
                key.to_uppercase()
            } else {
                key.to_owned()
            };
            self.command(&key, window, cx);
        }

        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    fn command(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .visual_range(cx)
            .is_some_and(|range| range != self.editor.read(cx).selected_range())
        {
            self.mode = Mode::Normal;
            self.reset_pending();
        }

        if let Ok(digit) = key.parse::<usize>()
            && key.len() == 1
            && (digit != 0 || self.count != 0)
        {
            self.count = (self.count * 10 + digit).min(10_000);
            return;
        }

        if key == "g" && !self.go {
            self.go = true;
            return;
        }

        if self.go && key != "g" {
            self.reset_pending();
            return;
        }

        let key = if self.go { "gg" } else { key };
        self.go = false;
        let explicit_count = self.count > 0;
        let count = std::mem::take(&mut self.count).max(1);
        let editor = self.editor.read(cx);
        let text = editor.text().clone();
        let cursor = match self.mode {
            Mode::Visual { cursor, .. } => cursor.min(text.len()),
            _ => normal_cursor(&text, editor.selected_range().start),
        };

        if let Mode::Visual {
            anchor,
            cursor,
            linewise,
        } = self.mode
            && matches!(key, "o" | "O")
        {
            self.mode = Mode::Visual {
                anchor: cursor,
                cursor: anchor,
                linewise,
            };
            self.desired_column = None;
            self.select(self.visual_range(cx).unwrap(), cx);
            return;
        }

        if let Some(range) = self.visual_range(cx)
            && matches!(key, "d" | "x" | "c" | "s" | "y")
        {
            let linewise = matches!(self.mode, Mode::Visual { linewise: true, .. });
            let operator = match key {
                "c" | "s" => 'c',
                "y" => 'y',
                _ => 'd',
            };
            self.operate(operator, range, linewise, window, cx);
            return;
        }

        let operator = self.operator.take();
        let count = (count * operator.map_or(1, |(_, count)| count)).min(10_000);

        if let Some((operator, _)) = operator
            && key == operator.to_string()
        {
            let row = (text.offset_to_point(cursor).row + count - 1).min(text.lines_len() - 1);
            self.operate(
                operator,
                lines(&text, cursor, text.line_start_offset(row)),
                true,
                window,
                cx,
            );
            return;
        }

        let motion_key = if operator.is_some_and(|(operator, _)| operator == 'c')
            && matches!(key, "w" | "W")
            && text.char_at(cursor).is_some_and(|ch| !ch.is_whitespace())
        {
            if key == "w" { "cw" } else { "cW" }
        } else {
            key
        };
        let motion_count =
            if key == "G" && !explicit_count && operator.is_none_or(|(_, count)| count == 1) {
                text.lines_len()
            } else {
                count
            };

        let vertical = matches!(key, "j" | "k" | "up" | "down");

        if let Some(mut movement) =
            motion(&text, cursor, motion_key, motion_count, operator.is_some())
        {
            if vertical
                && text.offset_to_point(movement.offset).row == text.offset_to_point(cursor).row
            {
                return;
            }

            let column = if vertical {
                Some(
                    self.desired_column
                        .unwrap_or_else(|| Column::at(&text, cursor, window, cx)),
                )
            } else if matches!(key, "$" | "end") {
                Some(Column::End)
            } else {
                None
            };

            if vertical {
                movement.offset = column.unwrap().offset(&text, movement.offset, window, cx);
            }

            if let Some((operator, _)) = operator {
                let range = if movement.linewise {
                    lines(&text, cursor, movement.offset)
                } else {
                    let end = cursor.max(movement.offset);
                    cursor.min(movement.offset)..if movement.inclusive {
                        next(&text, end).min(line(&text, end).end)
                    } else {
                        end
                    }
                };
                let linewise = movement.linewise
                    || (matches!(key, "w" | "W" | "b" | "B")
                        && range.start < range.end
                        && range.start == line(&text, range.start).start
                        && range.end == line(&text, range.end).start);
                self.operate(operator, range, linewise, window, cx);

                if operator == 'y' && linewise && movement.offset < cursor {
                    self.normal(movement.offset, cx);
                }
            } else if let Mode::Visual {
                anchor, linewise, ..
            } = self.mode
            {
                self.mode = Mode::Visual {
                    anchor,
                    cursor: normal_cursor(&text, movement.offset),
                    linewise,
                };
                self.select(self.visual_range(cx).unwrap(), cx);
            } else {
                self.normal(movement.offset, cx);
            }

            if operator.is_none() {
                self.desired_column = column;
            }

            return;
        }

        if operator.is_some() {
            return;
        }

        match key {
            "i" => self.insert(cursor, cx),
            "a" => self.insert(next(&text, cursor).min(line(&text, cursor).end), cx),
            "I" => self.insert(first_nonblank(&text, cursor), cx),
            "A" => self.insert(line(&text, cursor).end, cx),
            "o" | "O" => {
                let position = if key == "o" {
                    line(&text, cursor).end
                } else {
                    line(&text, cursor).start
                };
                let newline = newline(&text, cursor);
                self.replace(position..position, newline, window, cx);
                self.insert(position + if key == "o" { newline.len() } else { 0 }, cx);
            }
            "v" | "V" => {
                let linewise = key == "V";

                if matches!(self.mode, Mode::Visual { linewise: current, .. } if current == linewise)
                {
                    self.normal(cursor, cx);
                } else {
                    let anchor = match self.mode {
                        Mode::Visual { anchor, .. } => anchor,
                        _ => cursor,
                    };
                    self.mode = Mode::Visual {
                        anchor,
                        cursor,
                        linewise,
                    };
                    self.select(self.visual_range(cx).unwrap(), cx);
                }
            }
            "d" | "c" | "y" => self.operator = Some((key.chars().next().unwrap(), count)),
            "x" | "s" => {
                let end = motion(&text, cursor, "l", count, false).unwrap().offset;
                self.operate(
                    if key == "s" { 'c' } else { 'd' },
                    cursor..end,
                    false,
                    window,
                    cx,
                );
            }
            "X" => {
                let start = motion(&text, cursor, "h", count, false).unwrap().offset;
                self.operate('d', start..cursor, false, window, cx);
            }
            "D" | "C" => {
                let row = (text.offset_to_point(cursor).row + count - 1).min(text.lines_len() - 1);
                let end = line(&text, text.line_start_offset(row)).end;
                self.operate(
                    if key == "C" { 'c' } else { 'd' },
                    cursor..end,
                    false,
                    window,
                    cx,
                );
            }
            "Y" | "S" => {
                let row = (text.offset_to_point(cursor).row + count - 1).min(text.lines_len() - 1);
                self.operate(
                    if key == "S" { 'c' } else { 'y' },
                    lines(&text, cursor, text.line_start_offset(row)),
                    true,
                    window,
                    cx,
                );
            }
            "p" | "P" => self.paste(key == "P", count, window, cx),
            "u" => {
                for _ in 0..count {
                    window.dispatch_action(Box::new(Undo), cx);
                }
            }
            "/" => self
                .editor
                .update(cx, |editor, cx| editor.open_search(false, cx)),
            "n" | "N" => {
                let matches = self
                    .editor
                    .read(cx)
                    .search_session()
                    .matcher
                    .matched_ranges();

                if !matches.is_empty() {
                    let index = if key == "n" {
                        (matches.partition_point(|range| range.start <= cursor) + count - 1)
                            % matches.len()
                    } else {
                        (matches.partition_point(|range| range.start < cursor) + matches.len()
                            - count % matches.len())
                            % matches.len()
                    };
                    let target = normal_cursor(&text, matches[index].start);

                    if let Mode::Visual {
                        anchor, linewise, ..
                    } = self.mode
                    {
                        self.mode = Mode::Visual {
                            anchor,
                            cursor: target,
                            linewise,
                        };
                        self.select(self.visual_range(cx).unwrap(), cx);
                    } else {
                        self.normal(target, cx);
                    }
                    self.desired_column = None;
                }
            }
            // Unrecognized Normal-mode keys must never become inserted text.
            _ => {}
        }

        if count > 1
            && matches!(key, "i" | "a" | "I" | "A" | "o" | "O")
            && self.mode == Mode::Insert
        {
            let editor = self.editor.read(cx);
            let separator = if matches!(key, "o" | "O") {
                newline(&text, cursor)
            } else {
                ""
            };
            self.insertion = Some(Insertion::new(
                editor.text().clone(),
                editor.cursor(),
                count,
                separator,
            ));
        }
    }
}

impl Render for Vim {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.enabled {
            return Empty.into_any_element();
        }

        div()
            .debug_selector(|| "vim-mode-indicator".into())
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(match self.mode {
                Mode::Normal => "NORMAL",
                Mode::Insert => "INSERT",
                Mode::Visual {
                    linewise: false, ..
                } => "VISUAL",
                Mode::Visual { linewise: true, .. } => "VISUAL LINE",
            })
            .into_any_element()
    }
}
