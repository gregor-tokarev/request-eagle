use gpui_kit::component::{
    button::*,
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

struct FieldRow {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
    description: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

pub(super) struct FieldsChanged(pub Vec<(String, String)>);

/// A request's editable key/value rows, including one trailing empty row.
pub(super) struct RequestFields {
    id: &'static str,
    rows: Vec<FieldRow>,
}

impl EventEmitter<FieldsChanged> for RequestFields {}

impl RequestFields {
    pub(super) fn new(id: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut fields = Self {
            id,
            rows: Vec::new(),
        };
        fields.append_row(window, cx);

        fields
    }

    fn append_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("Key"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("Description"));
        let subscriptions = [&key, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        if this.rows.last().is_some_and(|row| {
                            !row.key.read(cx).value().is_empty()
                                || !row.value.read(cx).value().is_empty()
                        }) {
                            this.append_row(window, cx);
                        }

                        this.emit_change(cx);
                    }
                })
            })
            .collect();

        self.rows.push(FieldRow {
            enabled: true,
            key,
            value,
            description,
            _subscriptions: subscriptions,
        });
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        let values = self
            .rows
            .iter()
            .filter_map(|row| {
                let key = row.key.read(cx).value();

                (row.enabled && !key.trim().is_empty())
                    .then(|| (key.to_string(), row.value.read(cx).value().to_string()))
            })
            .collect();

        cx.emit(FieldsChanged(values));
        cx.notify();
    }
}

impl Render for RequestFields {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;

        v_flex()
            .debug_selector(move || format!("{id}-table"))
            .w_full()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(30.))
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(36.)).flex_none())
                    .children(["Key", "Value"].map(|label| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .flex()
                            .items_center()
                            .child(label)
                    }))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .child("Description"),
                            )
                            .child(
                                Button::new("bulk-edit")
                                    .ghost()
                                    .xsmall()
                                    .label("Bulk Edit")
                                    .disabled(true),
                            )
                            .when(id == "headers", |this| {
                                this.child(
                                    Button::new("header-presets")
                                        .ghost()
                                        .xsmall()
                                        .label("Presets")
                                        .child(Icon::new(IconName::ChevronDown).size(px(11.)))
                                        .disabled(true),
                                )
                            }),
                    )
                    .child(
                        Button::new("field-options")
                            .ghost()
                            .xsmall()
                            .w(px(30.))
                            .icon(IconName::Ellipsis)
                            .disabled(true),
                    ),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                let populated =
                    !row.key.read(cx).value().is_empty() || !row.value.read(cx).value().is_empty();

                h_flex()
                    .h(px(32.))
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(div().w(px(36.)).flex_none().flex().justify_center().when(
                        populated,
                        |this| {
                            this.child(
                                Checkbox::new(("enabled", index))
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
                            div()
                                .debug_selector(move || format!("{id}-{column}-{index}"))
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .border_r_1()
                                .border_color(cx.theme().border)
                                .child(
                                    Input::new(input)
                                        .small()
                                        .appearance(false)
                                        .aria_label(format!("{id} {column} {}", index + 1)),
                                )
                        }),
                    )
                    .child(div().w(px(30.)).flex_none().when(populated, |this| {
                        this.child(
                            Button::new(("remove-row", index))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .accessibility_label("Remove row")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rows.remove(index);
                                    this.emit_change(cx);
                                })),
                        )
                    }))
            }))
    }
}
