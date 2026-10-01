use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use aho_corasick::AhoCorasick;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ServerSentEvent;

const PREVIEW_CHARS: usize = 200;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum StreamState {
    Open,
    /// Stop was requested, and the response is completing.
    Stopping,
    Ended,
    Stopped,
    Failed(SharedString),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Event,
    Ended,
    Error,
}

struct Entry {
    kind: EntryKind,
    time: SharedString,
    /// The event type. Empty for the row that ends the stream.
    event: SharedString,
    id: SharedString,
    /// One line of the data, prepared once so rows render cheaply.
    preview: SharedString,
    data: SharedString,
}

/// The events of an event-stream response, newest first, with search and a
/// filter by event type.
pub(crate) struct EventLog {
    pub(super) state: StreamState,
    /// In arrival order.
    entries: Vec<Entry>,
    /// Event types in the order they first arrived.
    types: Vec<SharedString>,
    /// Indices of the entries that pass the filter and search, oldest first.
    shown: Vec<usize>,
    /// The details of expanded entries: their data, indented when it is JSON.
    expanded: HashMap<usize, SharedString>,
    /// The event type shown, or every type.
    filter: Option<SharedString>,
    matcher: Option<AhoCorasick>,
    search: Entity<InputState>,
    list: ListState,
    _search_subscription: Subscription,
}

impl EventLog {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search events"));
        let subscription = cx.subscribe(&search, |this, search, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = search.read(cx).value();
                let query = query.trim();
                this.matcher = (!query.is_empty())
                    .then(|| {
                        AhoCorasick::builder()
                            .ascii_case_insensitive(true)
                            .build([query])
                            .ok()
                    })
                    .flatten();
                this.refresh_list(cx);
            }
        });

        Self {
            state: StreamState::Open,
            entries: Vec::new(),
            types: Vec::new(),
            shown: Vec::new(),
            expanded: HashMap::new(),
            filter: None,
            matcher: None,
            search,
            list: ListState::new(0, ListAlignment::Top, px(0.)),
            _search_subscription: subscription,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        matches!(self.state, StreamState::Open | StreamState::Stopping)
    }

    pub(super) fn push(&mut self, events: Vec<ServerSentEvent>, cx: &mut Context<Self>) {
        for event in events {
            let event_type: SharedString = event.event.into();
            if !self.types.contains(&event_type) {
                self.types.push(event_type.clone());
            }

            self.add(Entry {
                kind: EntryKind::Event,
                time: time_label(event.time),
                event: event_type,
                id: event.id.into(),
                preview: preview(&event.data),
                data: event.data.into(),
            });
        }

        cx.notify();
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if self.state == StreamState::Open {
            self.state = StreamState::Stopping;
            cx.notify();
        }
    }

    /// The stream ended, or broke with `error`.
    pub(super) fn finish(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        let (state, kind, message) = match error {
            Some(error) => (StreamState::Failed(error.clone()), EntryKind::Error, error),
            None if self.state == StreamState::Stopping => {
                (StreamState::Stopped, EntryKind::Ended, "Stopped".into())
            }
            None => (
                StreamState::Ended,
                EntryKind::Ended,
                "The server ended the stream".into(),
            ),
        };

        self.state = state;
        self.add(Entry {
            kind,
            time: time_label(SystemTime::now()),
            event: SharedString::default(),
            id: SharedString::default(),
            preview: message.clone(),
            data: message,
        });
        cx.notify();
    }

    fn add(&mut self, entry: Entry) {
        let index = self.entries.len();
        self.entries.push(entry);

        if self.shows(index) {
            self.shown.push(index);

            // The newest row is first. Keep it in view unless the list was
            // scrolled down to read earlier events.
            let top = self.list.logical_scroll_top();
            let at_top = top.item_ix == 0 && top.offset_in_item <= px(0.);
            self.list.splice(0..0, 1);

            if at_top {
                self.list.scroll_to(ListOffset::default());
            }
        }
    }

    fn shows(&self, index: usize) -> bool {
        let entry = &self.entries[index];

        // Rows that end the stream show with every type.
        let kind = entry.kind != EntryKind::Event
            || self
                .filter
                .as_ref()
                .is_none_or(|filter| *filter == entry.event);

        kind && self.matcher.as_ref().is_none_or(|matcher| {
            matcher.is_match(entry.event.as_ref()) || matcher.is_match(entry.data.as_ref())
        })
    }

    fn refresh_list(&mut self, cx: &mut Context<Self>) {
        self.shown = (0..self.entries.len())
            .filter(|index| self.shows(*index))
            .collect();
        self.list.reset(self.shown.len());
        cx.notify();
    }

    fn set_filter(&mut self, filter: Option<SharedString>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refresh_list(cx);
    }

    /// The entry at a position of the list, which shows the newest first.
    fn entry_at(&self, position: usize) -> Option<usize> {
        self.shown
            .len()
            .checked_sub(position + 1)
            .map(|index| self.shown[index])
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.expanded.remove(&index).is_none() {
            let data = &self.entries[index].data;
            let detail = pretty_json(data).map_or_else(|| data.clone(), Into::into);
            self.expanded.insert(index, detail);
        }

        // Measure the row again at its new height.
        if let Ok(found) = self.shown.binary_search(&index) {
            let position = self.shown.len() - 1 - found;
            self.list.splice(position..position + 1, 1);
        }

        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let filter = self.filter.clone();
        let types = self.types.clone();
        let events = self
            .entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Event)
            .count();

        h_flex()
            .flex_none()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "response-events-search".into())
                    .w(rems(14.))
                    .min_w_0()
                    .child(
                        Input::new(&self.search)
                            .small()
                            .prefix(IconName::Search)
                            .cleanable(true)
                            .aria_label("Search events"),
                    ),
            )
            .child(
                Button::new("response-events-filter")
                    .debug_selector(|| "response-events-filter".into())
                    .ghost()
                    .small()
                    .label(
                        filter
                            .clone()
                            .unwrap_or_else(|| SharedString::from("All events")),
                    )
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |mut menu, _, _| {
                        let options = [None].into_iter().chain(types.iter().cloned().map(Some));

                        for option in options {
                            let view = view.clone();
                            let checked = option == filter;
                            let label = option
                                .clone()
                                .unwrap_or_else(|| SharedString::from("All events"));

                            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                                move |_, _, cx| {
                                    let option = option.clone();
                                    let _ = view.update(cx, |log, cx| log.set_filter(option, cx));
                                },
                            ));
                        }

                        menu
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "response-events-count".into())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(match events {
                        1 => "1 event".to_owned(),
                        count => format!("{count} events"),
                    }),
            )
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().downgrade();

        list(self.list.clone(), move |position, _, cx| {
            // Rows are built as they scroll into view.
            let Some(view) = view.upgrade() else {
                return div().into_any_element();
            };
            let log = view.read(cx);
            let Some(index) = log.entry_at(position) else {
                return div().into_any_element();
            };
            let entry = &log.entries[index];
            let detail = log.expanded.get(&index).cloned();
            let is_event = entry.kind == EntryKind::Event;
            let (icon, color) = match entry.kind {
                EntryKind::Event => (Icon::new(IconName::ArrowDown), cx.theme().info),
                EntryKind::Ended => (
                    Icon::new(IconName::CircleCheck),
                    cx.theme().muted_foreground,
                ),
                EntryKind::Error => (
                    Icon::default().path("icons/circle-alert.svg"),
                    cx.theme().danger,
                ),
            };
            let toggle = view.downgrade();

            v_flex()
                .id(("response-event", index))
                .debug_selector(move || format!("response-event-{position}"))
                .w_full()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .id(("response-event-summary", index))
                        .h_9()
                        .px_2()
                        .gap_3()
                        .when(is_event, |row| {
                            row.cursor_pointer()
                                .hover(|row| row.bg(cx.theme().muted))
                                .on_click(move |_, _, cx| {
                                    let _ = toggle.update(cx, |log, cx| log.toggle(index, cx));
                                })
                        })
                        .child(icon.size_4().flex_none().text_color(color))
                        .when(is_event, |row| {
                            row.child(
                                div()
                                    .debug_selector(move || {
                                        format!("response-event-type-{position}")
                                    })
                                    .flex_none()
                                    .max_w(rems(10.))
                                    .px_1()
                                    .rounded(cx.theme().radius_tokens().sm)
                                    .bg(cx.theme().muted)
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(entry.event.clone()),
                            )
                        })
                        .child(
                            div()
                                .debug_selector(move || format!("response-event-data-{position}"))
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .when(is_event, |text| {
                                    text.font_family(cx.theme().mono_font_family.clone())
                                        .text_xs()
                                })
                                .when(entry.kind == EntryKind::Error, |text| {
                                    text.text_color(cx.theme().danger)
                                })
                                .child(entry.preview.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(entry.time.clone()),
                        )
                        .child(div().flex_none().size_4().when(is_event, |slot| {
                            slot.child(
                                Icon::new(if detail.is_some() {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                })
                                .size_4()
                                .text_color(cx.theme().muted_foreground),
                            )
                        })),
                )
                .when_some(detail, |row, detail| {
                    let copied = entry.data.clone();

                    row.child(
                        h_flex()
                            .items_start()
                            .px_2()
                            .pb_2()
                            .gap_2()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .when(!entry.id.is_empty(), |details| {
                                        details.child(
                                            div()
                                                .debug_selector(move || {
                                                    format!("response-event-id-{position}")
                                                })
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .cursor_text()
                                                .child(SelectableText::new(
                                                    ("response-event-id", index),
                                                    format!("ID: {}", entry.id),
                                                )),
                                        )
                                    })
                                    .child(
                                        div()
                                            .debug_selector(move || {
                                                format!("response-event-detail-{position}")
                                            })
                                            .p_2()
                                            .rounded(cx.theme().radius_tokens().md)
                                            .bg(cx.theme().muted)
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .text_xs()
                                            .cursor_text()
                                            .child(
                                                SelectableText::new(
                                                    ("response-event-detail", index),
                                                    detail,
                                                )
                                                .document_order(1),
                                            ),
                                    ),
                            )
                            .child(
                                Button::new(("response-event-copy", index))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .accessibility_label("Copy event data")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copied.to_string(),
                                        ));
                                    }),
                            ),
                    )
                })
                .into_any_element()
        })
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn empty_state(&self) -> AnyElement {
        let media = EmptyMedia::new().with_variant(EmptyMediaVariant::Icon);
        let header = if self.is_open() && self.entries.is_empty() {
            EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Loader).with_animation(
                    "response-events-waiting",
                    Animation::new(Duration::from_secs(1)).repeat(),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                )))
                .title(EmptyTitle::new().child("Waiting for events…"))
        } else {
            EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Search)))
                .title(EmptyTitle::new().child("No matching events"))
        };

        div()
            .debug_selector(|| "response-events-empty".into())
            .flex()
            .flex_1()
            .min_h_0()
            .child(Empty::new().header(header))
            .into_any_element()
    }
}

#[cfg(test)]
impl EventLog {
    /// The event type and preview of each shown row, newest first.
    pub(crate) fn rows(&self) -> Vec<(String, String)> {
        self.shown
            .iter()
            .rev()
            .map(|&index| {
                let entry = &self.entries[index];
                (entry.event.to_string(), entry.preview.to_string())
            })
            .collect()
    }
}

impl Render for EventLog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "response-events".into())
            .flex_1()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(self.toolbar(cx))
            .child(if self.shown.is_empty() {
                self.empty_state()
            } else {
                self.list(cx)
            })
    }
}

/// Local wall-clock time with milliseconds, since events often arrive close together.
fn time_label(time: SystemTime) -> SharedString {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%H:%M:%S%.3f")
        .to_string()
        .into()
}

/// The start of the data on one line.
fn preview(data: &str) -> SharedString {
    let mut preview = String::new();

    for (index, character) in data.trim().chars().enumerate() {
        if index == PREVIEW_CHARS {
            preview.push('…');
            break;
        }

        preview.push(if character.is_whitespace() {
            ' '
        } else {
            character
        });
    }

    preview.into()
}

fn pretty_json(text: &str) -> Option<String> {
    if !text.trim_start().starts_with(['{', '[']) || super::exceeds_editor_limit(text) {
        return None;
    }

    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
}
