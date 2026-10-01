use crate::{
    variable_input::{VariableInput, VariableTarget, with_variables},
    variables::VariableScope,
};
use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

struct PathVariableRow {
    name: SharedString,
    value: Entity<InputState>,
    description: Entity<InputState>,
    completion: Entity<VariableInput>,
    _subscription: Subscription,
}

/// A path variable's value was edited.
pub(crate) struct PathVariableChanged {
    pub name: String,
    pub value: String,
}

/// The values of the URL's `:name` path variables. The URL names the rows,
/// so only their values and descriptions are editable.
pub(crate) struct PathVariables {
    rows: Vec<PathVariableRow>,
    scope: Entity<VariableScope>,
}

impl EventEmitter<PathVariableChanged> for PathVariables {}

impl PathVariables {
    pub(crate) fn new(
        names: &[String],
        values: &[(String, String)],
        scope: Entity<VariableScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut table = Self {
            rows: Vec::new(),
            scope,
        };
        table.set_names(names, values, window, cx);

        table
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Show a row for each name. Rows of names that remain keep what was
    /// typed in them; new rows start with their value from `values`.
    pub(crate) fn set_names(
        &mut self,
        names: &[String],
        values: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .rows
            .iter()
            .map(|row| row.name.as_ref())
            .eq(names.iter().map(String::as_str))
        {
            return;
        }

        let mut previous = std::mem::take(&mut self.rows);
        for name in names {
            let row = match previous.iter().position(|row| row.name.as_ref() == name) {
                Some(index) => previous.remove(index),
                None => {
                    let value = values
                        .iter()
                        .find(|(key, _)| key == name)
                        .map_or("", |(_, value)| value.as_str());
                    self.new_row(name, value, window, cx)
                }
            };
            self.rows.push(row);
        }

        cx.notify();
    }

    fn new_row(
        &self,
        name: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PathVariableRow {
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value.to_owned())
        });
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("Description"));
        let completion = cx.new(|cx| {
            VariableInput::new(
                VariableTarget::Input(value.clone()),
                self.scope.clone(),
                window,
                cx,
            )
        });
        let changed = name.to_owned();
        let subscription = cx.subscribe(&value, move |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.emit(PathVariableChanged {
                    name: changed.clone(),
                    value: input.read(cx).value().to_string(),
                });
            }
        });

        PathVariableRow {
            name: name.to_owned().into(),
            value,
            description,
            completion,
            _subscription: subscription,
        }
    }
}

impl Render for PathVariables {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "path-variables-table".into())
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
                    // Line the columns up with the Query Params table above.
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
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                h_flex()
                    .id(("path-variable", row.value.entity_id()))
                    .h_8()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .hover(|row| row.bg(cx.theme().table_hover))
                    .child(div().w_9().flex_none())
                    .child(
                        div()
                            .debug_selector(move || format!("path-variables-key-{index}"))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().table_head)
                            .overflow_hidden()
                            // Padding inside the cell keeps the columns as wide
                            // as the header's and the Query Params table's.
                            .child(
                                div()
                                    .px_2()
                                    .child(SelectableText::new("name", row.name.clone())),
                            ),
                    )
                    .children(
                        [("value", &row.value), ("description", &row.description)].map(
                            |(column, input)| {
                                let field = Input::new(input)
                                    .small()
                                    .appearance(false)
                                    .aria_label(format!("Path variable {} {column}", row.name));

                                div()
                                    .debug_selector(move || {
                                        format!("path-variables-{column}-{index}")
                                    })
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .when(column == "value", |cell| cell.border_r_1())
                                    .border_color(cx.theme().border)
                                    .child(if column == "value" {
                                        with_variables(&row.completion, field).into_any_element()
                                    } else {
                                        field.into_any_element()
                                    })
                            },
                        ),
                    )
            }))
    }
}
