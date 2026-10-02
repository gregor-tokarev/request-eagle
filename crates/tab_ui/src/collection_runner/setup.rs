use std::{ops::Range, rc::Rc};

use gpui_kit::component::{
    button::*,
    checkbox::Checkbox,
    input::NumberInput,
    scroll::{ScrollableElement as _, Scrollbar},
    tooltip::Tooltip,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use super::runner::{CollectionRunner, RunOptions};

/// A request of the run sequence being dragged to another place.
#[derive(Clone)]
pub(super) struct DraggedRequest {
    index: usize,
    label: SharedString,
}

impl Render for DraggedRequest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded(cx.theme().radius_tokens().md)
            .bg(cx.theme().secondary)
            .text_color(cx.theme().secondary_foreground)
            .text_sm()
            .child(self.label.clone())
    }
}

/// An advanced setting: its label, what it does, and the option it changes.
struct Setting {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    value: fn(&mut RunOptions) -> &mut bool,
}

const SETTINGS: [Setting; 6] = [
    Setting {
        id: "persist-responses",
        label: "Persist responses for a session",
        description: "Keep each response's headers and body to view after the run.",
        value: |options| &mut options.persist_responses,
    },
    Setting {
        id: "turn-off-logs",
        label: "Turn off logs during run",
        description: "Leave out the scripts' console output, which keeps long runs light.",
        value: |options| &mut options.logs_off,
    },
    Setting {
        id: "stop-on-error",
        label: "Stop run if an error occurs",
        description: "End the run when a request cannot be sent or a script fails. Failed tests do not stop it.",
        value: |options| &mut options.stop_on_error,
    },
    Setting {
        id: "keep-variable-values",
        label: "Keep variable values",
        description: "Keep the variables that scripts change for the rest of the session. Otherwise the run changes a copy.",
        value: |options| &mut options.keep_variables,
    },
    Setting {
        id: "without-cookies",
        label: "Run collection without using stored cookies",
        description: "Start the run with an empty cookie jar.",
        value: |options| &mut options.without_cookies,
    },
    Setting {
        id: "save-cookies",
        label: "Save cookies after collection run",
        description: "Keep the cookies that responses set during the run in the cookie jar.",
        value: |options| &mut options.save_cookies,
    },
];

/// An icon whose tooltip explains the control beside it.
fn info(id: impl Into<ElementId>, text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(IconName::Info).size_3p5())
        .tooltip(move |window, cx| Tooltip::new(text).build(window, cx))
}

/// A configuration field's label, with what it means.
fn label(text: &'static str, id: &'static str, tooltip: &'static str, cx: &App) -> Div {
    h_flex()
        .gap_1()
        .text_color(cx.theme().muted_foreground)
        .font_weight(FontWeight::MEDIUM)
        .child(text)
        .child(info(id, tooltip, cx))
}

impl CollectionRunner {
    pub(super) fn setup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .debug_selector(|| "runner-setup".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .items_start()
            .text_sm()
            .child(self.sequence(cx))
            .child(self.configuration(window, cx))
            .into_any_element()
    }

    /// A request of the run sequence, with its place among the selected
    /// requests while it is selected.
    fn sequence_row(
        &self,
        index: usize,
        number: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let item = &self.sequence[index];
        let request = &item.request;
        let selected = item.selected;
        let id = request.id.clone();
        let drag = DraggedRequest {
            index,
            label: request.name.clone(),
        };

        div()
            .id(ElementId::Name(format!("run-sequence-{id}").into()))
            .debug_selector(move || format!("run-sequence-{index}"))
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .size_full()
                    .px_2()
                    .gap_3()
                    .rounded(theme.radius_tokens().md)
                    .hover(|row| row.bg(theme.muted.opacity(0.5)))
                    .child(
                        div()
                            .flex_none()
                            .w_6()
                            .text_right()
                            .text_color(theme.muted_foreground)
                            .when_some(number, |lane, number| lane.child(number.to_string())),
                    )
                    .child(
                        Checkbox::new(ElementId::Name(format!("run-sequence-check-{id}").into()))
                            .accessibility_label(format!("Run {}", request.name))
                            .checked(selected)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Some(item) = this.sequence.get_mut(index) {
                                    item.selected = *checked;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(rems(3.5))
                            .child(method_label(request.request.method.as_str(), cx)),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .overflow_hidden()
                            .children(request.folders.iter().map(|folder| {
                                h_flex()
                                    .flex_none()
                                    .gap_1()
                                    .text_color(theme.muted_foreground)
                                    .child(folder.clone())
                                    .child(Icon::new(IconName::ChevronRight).size_3())
                            }))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .when(!selected, |name| name.text_color(theme.muted_foreground))
                                    .child(request.name.clone()),
                            ),
                    )
                    .child(
                        Icon::default()
                            .path("icons/grip-vertical.svg")
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    ),
            )
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .drag_over::<DraggedRequest>(move |row, drag: &DraggedRequest, _, cx| {
                if drag.index == index {
                    row
                } else {
                    row.bg(cx.theme().info.opacity(0.2))
                }
            })
            .on_drop(cx.listener(move |this, drag: &DraggedRequest, _, cx| {
                this.move_item(drag.index, index, cx);
            }))
            .into_any_element()
    }

    fn sequence(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        // Only selected requests are numbered. Rows show what the list
        // scrolls to, so a large collection stays quick.
        let mut number = 0;
        let numbers: Rc<[Option<usize>]> = self
            .sequence
            .iter()
            .map(|item| {
                number += usize::from(item.selected);
                item.selected.then_some(number)
            })
            .collect();
        let empty = numbers.is_empty();

        v_flex()
            .debug_selector(|| "run-sequence".into())
            .flex_1()
            .min_w(rems(9.))
            .h_full()
            .border_r_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .flex_none()
                    .h_10()
                    .px_4()
                    .gap_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child("Run Sequence"),
                    )
                    .child(
                        Button::new("deselect-all")
                            .debug_selector(|| "run-sequence-deselect-all".into())
                            .ghost()
                            .small()
                            .flex_none()
                            .label("Deselect All")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.select_all(false, cx))),
                    )
                    .child(divider(cx))
                    .child(
                        Button::new("select-all")
                            .debug_selector(|| "run-sequence-select-all".into())
                            .ghost()
                            .small()
                            .flex_none()
                            .label("Select All")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.select_all(true, cx))),
                    )
                    .child(divider(cx))
                    .child(
                        Button::new("reset-sequence")
                            .debug_selector(|| "run-sequence-reset".into())
                            .ghost()
                            .small()
                            .flex_none()
                            .label("Reset")
                            .tooltip("Restore the collection's order and select every request")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.reset_sequence(cx))),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "run-sequence-list".into())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .pb_2()
                    .child(if empty {
                        div()
                            .px_4()
                            .py_2()
                            .text_color(theme.muted_foreground)
                            .child("There are no HTTP requests to run here.")
                            .into_any_element()
                    } else {
                        uniform_list(
                            "run-sequence-list",
                            numbers.len(),
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                range
                                    .map(|index| this.sequence_row(index, numbers[index], cx))
                                    .collect()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.sequence_scroll)
                        .into_any_element()
                    })
                    .when(!empty, |list| {
                        list.child(Scrollbar::vertical(&self.sequence_scroll))
                    }),
            )
            .when(self.other_protocols > 0, |pane| {
                pane.child(
                    div()
                        .flex_none()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(match self.other_protocols {
                            1 => "1 gRPC or WebSocket request is left out. The runner sends HTTP requests.".to_owned(),
                            count => format!(
                                "{count} gRPC and WebSocket requests are left out. The runner sends HTTP requests."
                            ),
                        }),
                )
            })
    }

    fn configuration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let iterations = self.iterations_state(window, cx);
        let delay = self.delay_state(window, cx);
        let selected = self.selected_count();
        let preparing = self.preparing.is_some();
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;

        let content = v_flex()
            .id("run-configuration")
            .size_full()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_4()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Run configuration"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label(
                        "Iterations",
                        "iterations-info",
                        "How many times the requests run. With a data file, each iteration uses its next row.",
                        cx,
                    ))
                    .child(
                        div()
                            .debug_selector(|| "run-iterations".into())
                            .child(NumberInput::new(&iterations)),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label(
                        "Delay",
                        "delay-info",
                        "How long to wait between requests, in milliseconds.",
                        cx,
                    ))
                    .child(
                        div()
                            .debug_selector(|| "run-delay".into())
                            .child(
                                NumberInput::new(&delay)
                                    .suffix(div().text_color(muted).child("ms")),
                            ),
                    ),
            )
            .child(self.data_field(cx))
            .child(self.advanced_settings(cx))
            .child(
                v_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        Button::new("start-run")
                            .debug_selector(|| "start-run".into())
                            .primary()
                            .icon(IconName::Play)
                            .label(if preparing {
                                format!("Preparing {} to run…", self.name)
                            } else {
                                format!("Run {}", self.name)
                            })
                            .loading(preparing)
                            .disabled(selected == 0 || preparing)
                            .tooltip(if selected == 0 {
                                "Select a request to run"
                            } else {
                                "Run the selected requests"
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
                    )
                    .when_some(self.start_error.clone(), |field, error| {
                        field.child(div().text_xs().text_color(danger).child(error))
                    }),
            );

        // The pane narrows with the window down to its minimum.
        div()
            .debug_selector(|| "run-configuration".into())
            .w(rems(26.))
            .min_w(rems(15.))
            .h_full()
            .child(content)
    }

    fn data_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();

        v_flex()
            .gap_1()
            .child(label(
                "Test data file",
                "data-file-info",
                "Each CSV row or JSON object gives one iteration its values, which requests use as {{variables}} and scripts read with pm.iterationData.",
                cx,
            ))
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("Only JSON and CSV files are accepted."),
            )
            .child(match &self.data {
                Some(data) => h_flex()
                    .debug_selector(|| "run-data-file".into())
                    .gap_2()
                    .child(Icon::new(IconName::FileText).text_color(theme.muted_foreground))
                    .child(div().min_w_0().truncate().child(data.name.clone()))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(match data.rows.len() {
                                1 => "1 row".to_owned(),
                                rows => format!("{rows} rows"),
                            }),
                    )
                    .child(
                        Button::new("remove-data-file")
                            .debug_selector(|| "remove-data-file".into())
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Remove the data file")
                            .on_click(cx.listener(|this, _, _, cx| this.remove_data_file(cx))),
                    )
                    .into_any_element(),
                None => h_flex()
                    .child(
                        Button::new("select-data-file")
                            .debug_selector(|| "select-data-file".into())
                            .outline()
                            .label("Select File")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_data_file(window, cx)),
                            ),
                    )
                    .into_any_element(),
            })
            .when_some(self.data_error.clone(), |field, error| {
                field.child(
                    div()
                        .debug_selector(|| "run-data-file-error".into())
                        .text_xs()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
    }

    fn advanced_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let open = self.advanced;

        v_flex()
            .gap_2()
            .items_start()
            .child(
                Button::new("advanced-settings")
                    .debug_selector(|| "advanced-settings".into())
                    .ghost()
                    .icon(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .label("Advanced settings")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    })),
            )
            .when(open, |section| {
                section.child(
                    v_flex()
                        .w_full()
                        .pl_2()
                        .gap_2()
                        .children(SETTINGS.iter().map(|setting| {
                            let mut options = self.options;
                            let checked = *(setting.value)(&mut options);
                            let value = setting.value;

                            h_flex()
                                .items_start()
                                .gap_1()
                                .child(
                                    Checkbox::new(setting.id)
                                        .min_w_0()
                                        .label(setting.label)
                                        .checked(checked)
                                        .on_click(cx.listener(
                                            move |this, checked: &bool, _, cx| {
                                                *value(&mut this.options) = *checked;
                                                cx.notify();
                                            },
                                        )),
                                )
                                // Level with the label's first line.
                                .child(div().mt_0p5().child(info(
                                    (setting.id, 1usize),
                                    setting.description,
                                    cx,
                                )))
                        })),
                )
            })
    }
}

fn divider(cx: &App) -> impl IntoElement {
    div().flex_none().w(px(1.)).h_4().bg(cx.theme().border)
}
