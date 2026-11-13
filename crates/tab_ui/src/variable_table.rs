use std::collections::HashSet;

use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

struct VariableRow {
    name: Entity<InputState>,
    value: Entity<InputState>,
    /// The value stays masked until revealed. Requests send it as it is.
    secret: bool,
    _subscriptions: Vec<Subscription>,
}

/// The rows with a name, in table order, and the names of the secret ones.
pub(crate) struct VariablesChanged(pub Vec<(String, String)>, pub HashSet<String>);

/// Editable collection or environment variables, including one trailing empty row.
pub(crate) struct VariableTable {
    /// Debug selectors are `{prefix}-table`, `{prefix}-{name_column}-{row}`,
    /// `{prefix}-value-{row}`, `{prefix}-secret-{row}` and `{prefix}-remove-{row}`.
    prefix: &'static str,
    name_column: &'static str,
    /// Whether rows show the toggle that makes a variable secret.
    secret_toggle: bool,
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
            secret_toggle: false,
            rows: Vec::new(),
        };

        for (name, value) in variables {
            table.append_row(name, value, false, window, cx);
        }
        table.append_row("", "", false, window, cx);

        table
    }

    /// A table whose rows can be marked secret. `secrets` are the names of
    /// the variables that start secret.
    pub(crate) fn with_secrets(
        prefix: &'static str,
        name_column: &'static str,
        variables: &[(String, String)],
        secrets: &HashSet<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut table = Self {
            prefix,
            name_column,
            secret_toggle: true,
            rows: Vec::new(),
        };

        for (name, value) in variables {
            table.append_row(name, value, secrets.contains(name), window, cx);
        }
        table.append_row("", "", false, window, cx);

        table
    }

    fn append_row(
        &mut self,
        name: &str,
        value: &str,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Add variable")
                .default_value(name.to_owned())
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value.to_owned())
                .masked(secret)
        });
        let subscriptions = [&name, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        if this.rows.last().is_some_and(|row| !row.is_empty(cx)) {
                            this.append_row("", "", false, window, cx);
                        }

                        this.emit_change(cx);
                    }
                })
            })
            .collect();

        self.rows.push(VariableRow {
            name,
            value,
            secret,
            _subscriptions: subscriptions,
        });
    }

    fn set_secret(
        &mut self,
        index: usize,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };

        row.secret = secret;
        row.value
            .update(cx, |value, cx| value.set_masked(secret, window, cx));

        self.emit_change(cx);
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        let mut variables = Vec::new();
        let mut secrets = HashSet::new();

        for row in &self.rows {
            let name = row.name.read(cx).value().trim().to_owned();
            if name.is_empty() {
                continue;
            }

            if row.secret {
                secrets.insert(name.clone());
            }

            variables.push((name, row.value.read(cx).value().to_string()));
        }

        cx.emit(VariablesChanged(variables, secrets));
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
        let secret_toggle = self.secret_toggle;

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
                let secret = row.secret;
                let show_secret_toggle = secret_toggle && (populated || secret);

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
                                        .when(column == "name" && show_secret_toggle, |input| {
                                            input.pr(rems(5.5))
                                        })
                                        .child(
                                            Input::new(input)
                                                .small()
                                                .appearance(false)
                                                .aria_label(format!(
                                                    "Variable {column} {}",
                                                    index + 1
                                                ))
                                                .when(column == "value" && secret, |input| {
                                                    input.mask_toggle()
                                                }),
                                        ),
                                )
                                .when(column == "name" && show_secret_toggle, |cell| {
                                    cell.child(
                                        h_flex()
                                            .debug_selector(move || {
                                                format!("{prefix}-secret-{index}")
                                            })
                                            .absolute()
                                            .right_1()
                                            .top_0()
                                            .h_full()
                                            .child(
                                                Toggle::new("secret-variable")
                                                    .xsmall()
                                                    .gap_1()
                                                    .px_1()
                                                    .checked(secret)
                                                    .icon(if secret {
                                                        gpui_kit::assets::IconName::Lock
                                                    } else {
                                                        gpui_kit::assets::IconName::LockOpen
                                                    })
                                                    .label("Secure")
                                                    .tooltip(if secret {
                                                        "Show the value as plain text"
                                                    } else {
                                                        "Hide the value on screen"
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, checked: &bool, window, cx| {
                                                            this.set_secret(
                                                                index, *checked, window, cx,
                                                            )
                                                        },
                                                    )),
                                            ),
                                    )
                                })
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
