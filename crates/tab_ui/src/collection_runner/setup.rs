use std::{ops::Range, rc::Rc};

use gpui_kit::component::{
    alert::Alert,
    button::*,
    checkbox::Checkbox,
    input::NumberInput,
    scroll::{ScrollableElement as _, Scrollbar},
    tooltip::Tooltip,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use super::detail::empty_state;
use super::results::request_title;
use super::runner::{CollectionRunner, RunOptions};

/// A request of the run sequence being dragged to another place, drawn
/// like its row next to the pointer.
#[derive(Clone)]
pub(super) struct DraggedRequest {
    index: usize,
    method: SharedString,
    label: SharedString,
    /// Where the pointer took hold of the row.
    offset: Point<Pixels>,
}

impl Render for DraggedRequest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // The preview starts where the row did, so the pointer's offset in
        // the row brings it to the pointer.
        div()
            .pl(self.offset.x + px(8.))
            .pt(self.offset.y + px(8.))
            .child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .rounded(theme.radius_tokens().md)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_md()
                    .text_sm()
                    .text_color(theme.popover_foreground)
                    .child(method_label(self.method.clone(), cx))
                    .child(self.label.clone()),
            )
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
        .tooltip(move |window, cx| wrapped_tooltip(text, window, cx))
}

/// A tooltip whose text wraps rather than running across the window.
pub(super) fn wrapped_tooltip(
    text: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let text = text.into();

    Tooltip::element(move |_, _| div().max_w(rems(20.)).child(text.clone())).build(window, cx)
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

/// The bar at the top of each pane, with the pane's name.
fn pane_header(name: &'static str, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .h_10()
        .px_4()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .child(name),
        )
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
            method: request.request.method.as_str().into(),
            label: request.name.clone(),
            offset: Point::default(),
        };

        div()
            .h_8()
            .w_full()
            .px_2()
            // A line between rows shows where the dragged request lands.
            .drag_over::<DraggedRequest>(move |slot, drag: &DraggedRequest, _, cx| {
                if drag.index < index {
                    slot.border_b_2().border_color(cx.theme().info)
                } else if drag.index > index {
                    slot.border_t_2().border_color(cx.theme().info)
                } else {
                    slot
                }
            })
            .on_drop(cx.listener(move |this, drag: &DraggedRequest, _, cx| {
                this.move_item(drag.index, index, cx);
            }))
            .child(
                h_flex()
                    .id(ElementId::Name(format!("run-sequence-{id}").into()))
                    .debug_selector(move || format!("run-sequence-{index}"))
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
                        // A request left out of the run fades as a whole.
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .when(!selected, |request| request.opacity(0.5))
                            .child(
                                div()
                                    .flex_none()
                                    .w(rems(3.5))
                                    .child(method_label(request.request.method.as_str(), cx)),
                            )
                            .child(
                                request_title(
                                    ("run-sequence-title", index),
                                    request,
                                    self.folder_depth(),
                                    cx,
                                )
                                .flex_1(),
                            ),
                    )
                    .child(
                        Icon::default()
                            .path("icons/grip-vertical.svg")
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    )
                    .on_drag(drag, |drag, offset, _, cx| {
                        cx.new(|_| DraggedRequest {
                            offset,
                            ..drag.clone()
                        })
                    }),
            )
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
        let selected = self.selected_count();
        let all_selected = selected == self.sequence.len();
        let left_out = match self.other_protocols {
            0 => None,
            1 => Some(
                "1 gRPC or WebSocket request is left out. The runner sends HTTP requests."
                    .to_owned(),
            ),
            count => Some(format!(
                "{count} gRPC and WebSocket requests are left out. The runner sends HTTP requests."
            )),
        };
        let kind = if self.path == self.collection {
            "collection"
        } else {
            "folder"
        };

        v_flex()
            .debug_selector(|| "run-sequence".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .border_r_1()
            .border_color(theme.border)
            .child(
                pane_header("Run sequence", cx)
                    .overflow_hidden()
                    .when(!empty, |header| {
                        header
                            .child(
                                div()
                                    .debug_selector(|| "run-sequence-count".into())
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!(
                                        "{selected} of {} selected",
                                        self.sequence.len()
                                    )),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("select-all")
                                    .debug_selector(move || {
                                        if all_selected {
                                            "run-sequence-deselect-all".into()
                                        } else {
                                            "run-sequence-select-all".into()
                                        }
                                    })
                                    .ghost()
                                    .small()
                                    .flex_none()
                                    .label(if all_selected {
                                        "Deselect All"
                                    } else {
                                        "Select All"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_all(!all_selected, cx)
                                    })),
                            )
                            .child(divider(cx))
                            .child(
                                Button::new("reset-sequence")
                                    .debug_selector(|| "run-sequence-reset".into())
                                    .ghost()
                                    .small()
                                    .flex_none()
                                    .label("Reset")
                                    .tooltip(
                                        "Restore the collection's order and select every request",
                                    )
                                    .disabled(self.is_original())
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reset_sequence(cx)),
                                    ),
                            )
                    }),
            )
            .when_some(left_out.clone().filter(|_| !empty), |pane, note| {
                pane.child(
                    h_flex()
                        .debug_selector(|| "run-sequence-left-out".into())
                        .flex_none()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(Icon::new(IconName::Info).size_3p5().flex_none())
                        .child(div().min_w_0().child(note)),
                )
            })
            .child(
                div()
                    .debug_selector(|| "run-sequence-list".into())
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(if empty {
                        match left_out {
                            Some(note) => empty_state(
                                Icon::new(IconName::Inbox).text_color(theme.muted_foreground),
                                "No HTTP requests to run",
                                Some(note.into()),
                            ),
                            None => empty_state(
                                Icon::new(IconName::Inbox).text_color(theme.muted_foreground),
                                "No requests yet",
                                Some(format!("Add requests to this {kind} to run them.").into()),
                            ),
                        }
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
                        .pt_1()
                        .pb_2()
                        .track_scroll(&self.sequence_scroll)
                        .into_any_element()
                    })
                    .when(!empty, |list| {
                        list.child(Scrollbar::vertical(&self.sequence_scroll))
                    }),
            )
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
        // Nothing here can run, so nothing here can be set.
        let nothing = self.sequence.is_empty();
        let muted = cx.theme().muted_foreground;

        let content = v_flex()
            .id("run-configuration")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_4()
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
                            .w(rems(10.))
                            .child(NumberInput::new(&iterations).disabled(nothing)),
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
                            .w(rems(10.))
                            .child(
                                NumberInput::new(&delay)
                                    .disabled(nothing)
                                    .suffix(div().text_color(muted).child("ms")),
                            ),
                    ),
            )
            .child(self.data_field(nothing, cx))
            .child(self.advanced_settings(nothing, cx))
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(
                        // Long names shorten so the button stays in the pane.
                        Button::new("start-run")
                            .debug_selector(|| "start-run".into())
                            .primary()
                            .min_w_0()
                            .max_w_full()
                            .icon(Icon::default().path("icons/square-play.svg"))
                            .label(if preparing {
                                format!("Preparing {} to run…", self.name)
                            } else {
                                format!("Run {}", self.name)
                            })
                            .loading(preparing)
                            .disabled(selected == 0 || preparing)
                            .tooltip(if selected == 0 {
                                "Select a request to run".to_owned()
                            } else {
                                format!("Run the selected requests of {}", self.title())
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
                    )
                    .when_some(self.start_error.clone(), |field, error| {
                        field.child(
                            div()
                                .debug_selector(|| "run-start-error".into())
                                .w_full()
                                .child(Alert::error("run-start-error", error).small()),
                        )
                    }),
            );

        // The configuration gives up its width first, down to its minimum,
        // so the requests stay readable.
        v_flex()
            .debug_selector(|| "run-configuration".into())
            .flex_none()
            .w(relative(0.38))
            .min_w(rems(15.))
            .max_w(rems(26.))
            .h_full()
            .child(pane_header("Run configuration", cx))
            .child(content)
    }

    fn data_field(&self, nothing: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();

        v_flex()
            .gap_1()
            .child(label(
                "Test data file",
                "data-file-info",
                "A CSV file whose first row names its columns, or a JSON array of objects. Each row gives one iteration its values, which requests use as {{variables}} and scripts read with pm.iterationData.",
                cx,
            ))
            .child(match &self.data {
                // As tall as the button it replaces, so nothing below moves.
                Some(data) => {
                    let name = data.name.clone();

                    h_flex()
                        .id("run-data-file")
                        .debug_selector(|| "run-data-file".into())
                        .h_8()
                        .w_full()
                        .min_w_0()
                        .pl_2()
                        .pr_1()
                        .gap_2()
                        .rounded(theme.radius_tokens().md)
                        .border_1()
                        .border_color(theme.input)
                        .child(
                            Icon::new(IconName::FileText)
                                .size_4()
                                .flex_none()
                                .text_color(theme.muted_foreground),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(data.name.clone()))
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
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
                                .flex_none()
                                .icon(IconName::Close)
                                .tooltip("Remove the data file")
                                .on_click(cx.listener(|this, _, _, cx| this.remove_data_file(cx))),
                        )
                        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
                        .into_any_element()
                }
                None => h_flex()
                    .child(
                        Button::new("select-data-file")
                            .debug_selector(|| "select-data-file".into())
                            .outline()
                            .label("Select File")
                            .disabled(nothing)
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

    fn advanced_settings(&self, nothing: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let open = self.advanced;
        let theme = cx.theme();

        v_flex()
            .gap_2()
            .items_start()
            .child(
                // Starts at the same edge as the other fields' labels.
                Button::new("advanced-settings")
                    .debug_selector(|| "advanced-settings".into())
                    .text()
                    .small()
                    .text_color(theme.muted_foreground)
                    .font_weight(FontWeight::MEDIUM)
                    .label("Advanced settings")
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size_3p5(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    })),
            )
            .when(open, |section| {
                section.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .children(SETTINGS.iter().map(|setting| {
                            let mut options = self.options;
                            let checked = *(setting.value)(&mut options);
                            let value = setting.value;

                            h_flex()
                                .w_full()
                                .items_start()
                                .gap_2()
                                .child(
                                    Checkbox::new(setting.id)
                                        .small()
                                        .min_w_0()
                                        .label(setting.label)
                                        .checked(checked)
                                        .disabled(nothing)
                                        .on_click(cx.listener(
                                            move |this, checked: &bool, _, cx| {
                                                *value(&mut this.options) = *checked;
                                                cx.notify();
                                            },
                                        )),
                                )
                                // The icons line up, level with the labels'
                                // first lines.
                                .child(div().flex_1())
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
