use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

struct VariableRow {
    name: Entity<InputState>,
    value: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

/// The rows with a name, in table order.
pub(crate) struct VariablesChanged(pub Vec<(String, String)>);

/// Editable collection or environment variables, including one trailing empty row.
pub(crate) struct VariableTable {
    /// Debug selectors are `{prefix}-table`, `{prefix}-{name_column}-{row}`,
    /// `{prefix}-value-{row}` and `{prefix}-remove-{row}`.
    prefix: &'static str,
    name_column: &'static str,
    rows: Vec<VariableRow>,
}

impl EventEmitter<VariablesChanged> for VariableTable {}

impl VariableTable {
    pub(crate) fn new(
        prefix: &'static str,
        name_column: &'static str,
        variables: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut table = Self {
            prefix,
            name_column,
            rows: Vec::new(),
        };

        for (name, value) in variables {
            table.append_row(name, value, window, cx);
        }
        table.append_row("", "", window, cx);

        table
    }

    fn append_row(&mut self, name: &str, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Add variable")
                .default_value(name.to_owned())
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value.to_owned())
        });
        let subscriptions = [&name, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        if this.rows.last().is_some_and(|row| !row.is_empty(cx)) {
                            this.append_row("", "", window, cx);
                        }

                        this.emit_change(cx);
                    }
                })
            })
            .collect();

        self.rows.push(VariableRow {
            name,
            value,
            _subscriptions: subscriptions,
        });
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        let variables = self
            .rows
            .iter()
            .filter_map(|row| {
                let name = row.name.read(cx).value().trim().to_owned();

                (!name.is_empty()).then(|| (name, row.value.read(cx).value().to_string()))
            })
            .collect();

        cx.emit(VariablesChanged(variables));
        cx.notify();
    }
}

impl VariableRow {
    fn is_empty(&self, cx: &App) -> bool {
        self.name.read(cx).value().is_empty() && self.value.read(cx).value().is_empty()
    }
}

impl Render for VariableTable {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (prefix, name_column) = (self.prefix, self.name_column);

        v_flex()
            .debug_selector(move || format!("{prefix}-table"))
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
                    .children(["Variable", "Value"].map(|label| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_2()
                            .when(label == "Variable", |cell| cell.border_r_1())
                            .border_color(cx.theme().border)
                            .flex()
                            .items_center()
                            .child(label)
                    })),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                let populated = !row.is_empty(cx);

                h_flex()
                    .id(("variable", row.name.entity_id()))
                    .h_8()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .hover(|row| row.bg(cx.theme().table_hover))
                    .children(
                        [
                            ("name", name_column, &row.name),
                            ("value", "value", &row.value),
                        ]
                        .map(|(column, selector, input)| {
                            h_flex()
                                .relative()
                                .debug_selector(move || format!("{prefix}-{selector}-{index}"))
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .when(column == "name", |cell| cell.border_r_1())
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .when(column == "value" && populated, |input| input.pr_7())
                                        .child(
                                            Input::new(input).small().appearance(false).aria_label(
                                                format!("Variable {column} {}", index + 1),
                                            ),
                                        ),
                                )
                                .when(column == "value" && populated, |cell| {
                                    cell.child(
                                        h_flex().absolute().right_1().top_0().h_full().child(
                                            Button::new("remove-variable")
                                                .debug_selector(move || {
                                                    format!("{prefix}-remove-{index}")
                                                })
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Close)
                                                .accessibility_label("Remove variable")
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
