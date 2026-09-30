use std::rc::Rc;

use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::view::{EntryKind, Filter, GrpcResponse};

/// A visible stream row: its entry index, and what it shows.
struct Row {
    index: usize,
    kind: EntryKind,
    time: SharedString,
    summary: SharedString,
    detail: Option<SharedString>,
    expanded: bool,
}

impl GrpcResponse {
    /// Sent and received messages and call events, newest first, with
    /// search, a direction filter and Clear Messages.
    pub(super) fn stream(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Rc<[Row]> = self
            .visible()
            .into_iter()
            .map(|index| {
                let entry = &self.entries[index];

                Row {
                    index,
                    kind: entry.kind,
                    time: time_label(entry.at).into(),
                    summary: entry.summary.clone(),
                    detail: entry.detail.clone(),
                    expanded: self.expanded.contains(&index),
                }
            })
            .collect();
        let view = cx.entity().downgrade();

        v_flex()
            .debug_selector(|| "grpc-stream".into())
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(self.stream_toolbar(cx))
            .when(self.hidden > 0, |stream| {
                stream.child(
                    h_flex()
                        .debug_selector(|| "grpc-hidden-messages".into())
                        .flex_none()
                        .h_8()
                        .px_2()
                        .gap_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::EyeOff))
                        .child(div().flex_1().child(format!(
                            "{} {} hidden",
                            self.hidden,
                            if self.hidden == 1 {
                                "message"
                            } else {
                                "messages"
                            }
                        )))
                        .child(
                            Button::new("grpc-restore-messages")
                                .debug_selector(|| "grpc-restore-messages".into())
                                .ghost()
                                .xsmall()
                                .label("Restore")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.hidden = 0;
                                    this.refresh_list();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(
                list(self.list.clone(), move |position, _, cx| {
                    let row = &rows[position];
                    let index = row.index;
                    let view = view.clone();
                    let (icon, color) = match row.kind {
                        EntryKind::Sent => (Icon::new(IconName::ArrowUp), cx.theme().warning),
                        EntryKind::Received => (Icon::new(IconName::ArrowDown), cx.theme().info),
                        EntryKind::Info => (Icon::new(IconName::Info), cx.theme().muted_foreground),
                        EntryKind::Completed => {
                            (Icon::new(IconName::CircleCheck), cx.theme().success)
                        }
                        EntryKind::Error => (
                            Icon::default().path("icons/circle-alert.svg"),
                            cx.theme().danger,
                        ),
                    };
                    let message = matches!(row.kind, EntryKind::Sent | EntryKind::Received);

                    v_flex()
                        .id(("grpc-stream-row", index))
                        .debug_selector(move || format!("grpc-stream-row-{position}"))
                        .w_full()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            h_flex()
                                .id(("grpc-stream-summary", index))
                                .h_9()
                                .px_2()
                                .gap_3()
                                .when(row.detail.is_some(), |summary| {
                                    summary
                                        .cursor_pointer()
                                        .hover(|row| row.bg(cx.theme().muted))
                                })
                                .child(icon.size_4().flex_none().text_color(color))
                                .child(
                                    div()
                                        .debug_selector(move || {
                                            format!("grpc-stream-summary-{position}")
                                        })
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .when(message, |text| {
                                            text.font_family(cx.theme().mono_font_family.clone())
                                                .text_xs()
                                        })
                                        .child(row.summary.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(row.time.clone()),
                                )
                                .child(div().flex_none().size_4().when(
                                    row.detail.is_some(),
                                    |slot| {
                                        slot.child(
                                            Icon::new(if row.expanded {
                                                IconName::ChevronUp
                                            } else {
                                                IconName::ChevronDown
                                            })
                                            .size_4()
                                            .text_color(cx.theme().muted_foreground),
                                        )
                                    },
                                ))
                                .when(row.detail.is_some(), |summary| {
                                    summary.on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |this, cx| this.toggle(index, cx));
                                    })
                                }),
                        )
                        .when_some(
                            row.detail.clone().filter(|_| row.expanded),
                            |row_view, detail| {
                                let copied = detail.clone();

                                row_view.child(
                                    h_flex()
                                        .items_start()
                                        .px_2()
                                        .pb_2()
                                        .gap_2()
                                        .child(
                                            div()
                                                .debug_selector(move || {
                                                    format!("grpc-stream-detail-{position}")
                                                })
                                                .flex_1()
                                                .min_w_0()
                                                .p_2()
                                                .rounded(cx.theme().radius_tokens().md)
                                                .bg(cx.theme().muted)
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .text_xs()
                                                .cursor_text()
                                                .child(SelectableText::new(
                                                    ("grpc-stream-detail", index),
                                                    detail,
                                                )),
                                        )
                                        .child(
                                            Button::new(("grpc-copy-message", index))
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Copy)
                                                .accessibility_label("Copy message")
                                                .on_click(move |_, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(
                                                            copied.to_string(),
                                                        ),
                                                    );
                                                }),
                                        ),
                                )
                            },
                        )
                        .into_any_element()
                })
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }

    fn stream_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let filter = self.filter;

        h_flex()
            .flex_none()
            .gap_2()
            .when_some(self.search.clone(), |toolbar, search| {
                toolbar.child(
                    div()
                        .debug_selector(|| "grpc-stream-search".into())
                        .w(rems(14.))
                        .child(Input::new(&search).small().aria_label("Search messages")),
                )
            })
            .child(
                Button::new("grpc-stream-filter")
                    .debug_selector(|| "grpc-stream-filter".into())
                    .ghost()
                    .small()
                    .label(match filter {
                        Filter::All => "All Messages",
                        Filter::Sent => "Sent Messages",
                        Filter::Received => "Received Messages",
                    })
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (option, label) in [
                            (Filter::Sent, "Sent Messages"),
                            (Filter::Received, "Received Messages"),
                            (Filter::All, "All Messages"),
                        ] {
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(option == filter)
                                    .on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |this, cx| {
                                            this.filter = option;
                                            this.refresh_list();
                                            cx.notify();
                                        });
                                    }),
                            );
                        }

                        menu
                    }),
            )
            .child(
                Button::new("grpc-clear-messages")
                    .debug_selector(|| "grpc-clear-messages".into())
                    .ghost()
                    .small()
                    .icon(Icon::default().path("icons/trash.svg"))
                    .label("Clear Messages")
                    .disabled(self.entries.len() == self.hidden)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hidden = this.entries.len();
                        this.expanded.clear();
                        this.refresh_list();
                        cx.notify();
                    })),
            )
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&index) {
            self.expanded.insert(index);
        }

        // Measure the row again at its new height.
        if let Some(position) = self.visible().iter().position(|visible| *visible == index) {
            self.list.splice(position..position + 1, 1);
        }

        cx.notify();
    }
}

/// Local wall-clock time, as the stream shows it.
fn time_label(at: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%H:%M:%S")
        .to_string()
}
