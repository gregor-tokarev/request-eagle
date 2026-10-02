use crate::{
    variable_input::{VariableInput, VariableTarget, with_variables},
    variables::VariableScope,
};
use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::FormPart;

struct FieldRow {
    enabled: bool,
    /// Whether the value is the path of a file to send.
    file: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
    description: Entity<InputState>,
    completions: [Entity<VariableInput>; 2],
    _subscriptions: Vec<Subscription>,
}

/// The rows that are sent, in table order.
pub(crate) struct FieldsChanged(pub Vec<(String, String)>);

/// Asks for a file to send from a row, whose value input takes its path.
pub(crate) struct ChooseFile(pub Entity<InputState>);

/// A request's editable key/value rows, including one trailing empty row.
pub(crate) struct RequestFields {
    id: &'static str,
    rows: Vec<FieldRow>,
    generated_headers: Vec<(SharedString, SharedString)>,
    focus: FocusHandle,
    scope: Entity<VariableScope>,
    /// Whether a row without a name is sent, as a query parameter `=value`
    /// is. Headers and metadata need a name.
    keyless_rows: bool,
    /// Whether a row can send a file instead of text, as in a multipart form.
    file_rows: bool,
}

impl EventEmitter<FieldsChanged> for RequestFields {}
impl EventEmitter<ChooseFile> for RequestFields {}

impl RequestFields {
    pub(crate) fn new(
        id: &'static str,
        values: &[(String, String)],
        generated_headers: &[(String, String)],
        scope: Entity<VariableScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut fields = Self {
            id,
            rows: Vec::new(),
            generated_headers: generated_headers
                .iter()
                .map(|(name, value)| (name.clone().into(), value.clone().into()))
                .collect(),
            focus: cx.focus_handle(),
            scope,
            keyless_rows: false,
            file_rows: false,
        };

        for (key, value) in values {
            fields.append_row(key, value, window, cx);
        }
        fields.append_row("", "", window, cx);

        fields
    }

    pub(crate) fn with_keyless_rows(mut self) -> Self {
        self.keyless_rows = true;
        self
    }

    /// The parts of a multipart form, whose rows can send files.
    pub(crate) fn multipart(
        id: &'static str,
        parts: &[FormPart],
        scope: Entity<VariableScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let values: Vec<_> = parts
            .iter()
            .map(|part| (part.name.clone(), part.value.clone()))
            .collect();
        let mut fields = Self::new(id, &values, &[], scope, window, cx).with_keyless_rows();
        fields.file_rows = true;

        for (row, part) in fields.rows.iter_mut().zip(parts) {
            row.file = part.file;
            if part.file {
                row.value.update(cx, |value, cx| {
                    value.set_placeholder("Choose a file", window, cx)
                });
            }
        }

        fields
    }

    /// The rows that are sent, as `FieldsChanged` lists them.
    pub(crate) fn values(&self, cx: &App) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter(|row| self.is_sent(row, cx))
            .map(|row| {
                (
                    row.key.read(cx).value().to_string(),
                    row.value.read(cx).value().to_string(),
                )
            })
            .collect()
    }

    /// The rows that are sent as the parts of a multipart form.
    pub(crate) fn parts(&self, cx: &App) -> Vec<FormPart> {
        self.rows
            .iter()
            .filter(|row| self.is_sent(row, cx))
            .map(|row| FormPart {
                name: row.key.read(cx).value().to_string(),
                value: row.value.read(cx).value().to_string(),
                file: row.file,
            })
            .collect()
    }

    /// Switch a row between text and a file. Its value no longer applies.
    fn set_file(&mut self, index: usize, file: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };
        if row.file == file {
            return;
        }

        row.file = file;
        row.value.update(cx, |value, cx| {
            value.set_placeholder(if file { "Choose a file" } else { "Value" }, window, cx);
            value.set_value("", window, cx);
        });
        self.emit_change(cx);
    }

    /// Enabled rows with a name are sent. With `keyless_rows`, any row with a
    /// key or a value is, as the URL's query keeps them.
    fn is_sent(&self, row: &FieldRow, cx: &App) -> bool {
        let key = row.key.read(cx).value();
        let named = if self.keyless_rows {
            !key.is_empty() || !row.value.read(cx).value().is_empty()
        } else {
            !key.trim().is_empty()
        };

        row.enabled && named
    }

    pub(crate) fn set_generated_headers(
        &mut self,
        headers: &[(String, String)],
        cx: &mut Context<Self>,
    ) {
        if self.generated_headers.len() == headers.len()
            && self
                .generated_headers
                .iter()
                .zip(headers)
                .all(|((name, value), (key, text))| name.as_ref() == key && value.as_ref() == text)
        {
            return;
        }

        self.generated_headers = headers
            .iter()
            .map(|(name, value)| (name.clone().into(), value.clone().into()))
            .collect();
        cx.notify();
    }

    /// Show `values` in the rows that are sent, as when the URL's query
    /// changed. Disabled and empty rows stay; other rows are updated, added
    /// before the empty row or removed to match.
    pub(crate) fn set_values(
        &mut self,
        values: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut values = values.iter();
        let mut index = 0;

        while index < self.rows.len() {
            let row = &self.rows[index];
            if !self.is_sent(row, cx) {
                index += 1;
                continue;
            }

            let Some((key, value)) = values.next() else {
                self.rows.remove(index);
                continue;
            };

            for (input, text) in [(&row.key, key), (&row.value, value)] {
                if input.read(cx).value() != text.as_str() {
                    input.update(cx, |input, cx| input.set_value(text.clone(), window, cx));
                }
            }
            index += 1;
        }

        for (key, value) in values {
            let row = self.new_row(key, value, window, cx);
            let empty = self.rows.len().saturating_sub(1);
            self.rows.insert(empty, row);
        }

        cx.notify();
    }

    fn append_row(&mut self, key: &str, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.new_row(key, value, window, cx);
        self.rows.push(row);
    }

    fn new_row(
        &mut self,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FieldRow {
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Key")
                .default_value(key.to_owned())
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value.to_owned())
        });
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("Description"));
        let completions = [&key, &value].map(|input| {
            cx.new(|cx| {
                VariableInput::new(
                    VariableTarget::Input(input.clone()),
                    self.scope.clone(),
                    window,
                    cx,
                )
            })
        });
        let subscriptions = [&key, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        if this.rows.last().is_some_and(|row| {
                            !row.key.read(cx).value().is_empty()
                                || !row.value.read(cx).value().is_empty()
                        }) {
                            this.append_row("", "", window, cx);
                        }

                        this.emit_change(cx);
                    }
                })
            })
            .collect();

        FieldRow {
            enabled: true,
            file: false,
            key,
            value,
            description,
            completions,
            _subscriptions: subscriptions,
        }
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        cx.emit(FieldsChanged(self.values(cx)));
        cx.notify();
    }
}

impl Render for RequestFields {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        let view = cx.entity().downgrade();

        v_flex()
            .id(id)
            .debug_selector(move || format!("{id}-table"))
            .track_focus(&self.focus)
            .w_full()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_tokens().lg)
            .overflow_hidden()
            .child(
                h_flex()
                    .h_8()
                    .bg(cx.theme().table_head)
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w_9().flex_none())
                    .children(["Key", "Value", "Description"].map(|label| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_2()
                            .when(label != "Description", |cell| cell.border_r_1())
                            .border_color(cx.theme().border)
                            .flex()
                            .items_center()
                            .child(label)
                    })),
            )
            .children(
                self.generated_headers
                    .iter()
                    .enumerate()
                    .map(|(index, (name, value))| {
                        h_flex()
                            .id(format!("generated-header-{name}"))
                            .debug_selector(move || format!("headers-generated-row-{index}"))
                            .min_h_8()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .capture_any_mouse_down(cx.listener(
                                |this, event: &MouseDownEvent, window, cx| {
                                    if event.button == MouseButton::Left {
                                        window.focus(&this.focus, cx);
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_mouse_move(cx.listener(|_, event: &MouseMoveEvent, _, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    cx.notify();
                                }
                            }))
                            .child(div().w_9().flex_none())
                            .children(
                                [
                                    ("key", name.clone()),
                                    ("value", value.clone()),
                                    (
                                        "description",
                                        SharedString::from(if name == "Cookie" {
                                            "From the cookie jar"
                                        } else {
                                            "Auto-generated"
                                        }),
                                    ),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(
                                    |(column_index, (column, text))| {
                                        div()
                                            .debug_selector(move || {
                                                format!("headers-generated-{column}-{index}")
                                            })
                                            .flex_1()
                                            .min_w_0()
                                            .px_2()
                                            .py_1()
                                            .when(column != "description", |cell| cell.border_r_1())
                                            .border_color(cx.theme().border)
                                            .cursor_text()
                                            // Generated headers are read-only.
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                SelectableText::new(column, text).document_order(
                                                    (index * 3 + column_index) as u64,
                                                ),
                                            )
                                    },
                                ),
                            )
                    }),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                let populated =
                    !row.key.read(cx).value().is_empty() || !row.value.read(cx).value().is_empty();
                let file = row.file;
                let view = view.clone();

                h_flex()
                    .id(("request-field", row.key.entity_id()))
                    .group("request-field-row")
                    .h_8()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .hover(|row| row.bg(cx.theme().table_hover))
                    .child(div().w_9().flex_none().flex().justify_center().when(
                        populated,
                        |this| {
                            this.child(
                                Checkbox::new("enabled")
                                    .debug_selector(move || format!("{id}-enabled-{index}"))
                                    .accessibility_label(format!("Enable {id} row {}", index + 1))
                                    .checked(row.enabled)
                                    .on_click(cx.listener(move |this, enabled, _, cx| {
                                        this.rows[index].enabled = *enabled;
                                        this.emit_change(cx);
                                    })),
                            )
                        },
                    ))
                    .children(
                        [
                            ("key", &row.key),
                            ("value", &row.value),
                            ("description", &row.description),
                        ]
                        .map(|(column, input)| {
                            h_flex()
                                .relative()
                                .debug_selector(move || format!("{id}-{column}-{index}"))
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .when(column != "description", |cell| cell.border_r_1())
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .when(column == "description" && populated, |input| {
                                            input.pr_7()
                                        })
                                        .child({
                                            let input = Input::new(input)
                                                .small()
                                                .appearance(false)
                                                .aria_label(format!("{id} {column} {}", index + 1));
                                            match column {
                                                "key" => with_variables(&row.completions[0], input)
                                                    .into_any_element(),
                                                "value" => {
                                                    with_variables(&row.completions[1], input)
                                                        .into_any_element()
                                                }
                                                _ => input.into_any_element(),
                                            }
                                        }),
                                )
                                // Whether the part sends text or a file.
                                .when(column == "key" && self.file_rows, |cell| {
                                    let view = view.clone();
                                    cell.child(
                                        Button::new("part-type")
                                            .debug_selector(move || format!("{id}-type-{index}"))
                                            .ghost()
                                            .small()
                                            .mr_1()
                                            .label(if file { "File" } else { "Text" })
                                            .icon(IconName::ChevronDown)
                                            .accessibility_label(format!(
                                                "{id} row {} sends {}",
                                                index + 1,
                                                if file { "a file" } else { "text" }
                                            ))
                                            .dropdown_menu(move |menu, _, _| {
                                                [("Text", false), ("File", true)].into_iter().fold(
                                                    menu,
                                                    |menu, (label, option)| {
                                                        let view = view.clone();
                                                        menu.item(
                                                            PopupMenuItem::new(label)
                                                                .checked(option == file)
                                                                .on_click(move |_, window, cx| {
                                                                    let _ = view.update(
                                                                        cx,
                                                                        |view, cx| {
                                                                            view.set_file(
                                                                                index, option,
                                                                                window, cx,
                                                                            )
                                                                        },
                                                                    );
                                                                }),
                                                        )
                                                    },
                                                )
                                            }),
                                    )
                                })
                                .when(column == "value" && file, |cell| {
                                    let input = input.clone();
                                    cell.child(
                                        Button::new("choose-file")
                                            .debug_selector(move || {
                                                format!("{id}-choose-file-{index}")
                                            })
                                            .ghost()
                                            .small()
                                            .mr_1()
                                            .icon(IconName::FolderOpen)
                                            .accessibility_label("Choose a file")
                                            .tooltip("Choose a file")
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.emit(ChooseFile(input.clone()));
                                            })),
                                    )
                                })
                                .when(column == "description" && populated, |cell| {
                                    cell.child(
                                        h_flex().absolute().right_1().top_0().h_full().child(
                                            Button::new("remove-row")
                                                .debug_selector(move || {
                                                    format!("{id}-remove-{index}")
                                                })
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Close)
                                                .accessibility_label("Remove row")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.rows.remove(index);
                                                    this.emit_change(cx);
                                                })),
                                        ),
                                    )
                                })
                        }),
                    )
            }))
    }
}
