use std::{collections::VecDeque, fmt::Write as _};

use aho_corasick::AhoCorasick;
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{EditorState, Input, InputEvent, InputState},
    kbd::Kbd,
    menu::{DropdownMenu, PopupMenuItem},
    resizable::{ResizableState, resizable_panel, v_resizable},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{
    WebSocketClose, WebSocketEvent, WebSocketEventKind, WebSocketHandshake, WebSocketMessage,
};

use super::draft::{ConnectionState, WebSocketDraft};
use crate::actions::SendRequest;
use crate::response_view::{ResponseBodyEditor, VirtualBody, exceeds_editor_limit, hex_dump};

/// A stream can run for hours. Keep the newest messages within both limits.
pub(super) const MAX_ENTRIES: usize = 50_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;

const PREVIEW_CHARS: usize = 200;
const ROW_HEIGHT: Rems = rems(2.);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryKind {
    Sent,
    Received,
    Connected,
    Disconnected,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Sent,
    Received,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All messages",
            Filter::Sent => "Sent",
            Filter::Received => "Received",
        }
    }
}

pub(super) struct Entry {
    pub(super) kind: EntryKind,
    time: SharedString,
    /// One line of the content, prepared once so rows render cheaply.
    pub(super) preview: SharedString,
    /// Message payload bytes. Connection events have none.
    size: Option<usize>,
    pub(super) content: Content,
}

pub(super) enum Content {
    Text(SharedString),
    Binary(Vec<u8>),
}

impl Content {
    fn len(&self) -> usize {
        match self {
            Content::Text(text) => text.len(),
            Content::Binary(bytes) => bytes.len(),
        }
    }
}

/// The selected entry, shown below the list.
struct Detail {
    id: u64,
    view: AnyView,
}

/// A connection's messages and events, newest first.
pub(crate) struct MessageLog {
    /// The draft whose connection the log shows.
    draft: WeakEntity<WebSocketDraft>,
    /// Oldest first; the entry at `first` is the front.
    pub(super) entries: VecDeque<Entry>,
    /// Entries have stable ids, so removing old ones keeps the selection.
    first: u64,
    bytes: usize,
    /// Entries removed to stay within the limits since the log was cleared.
    pub(super) dropped: usize,
    /// Ids of the entries that pass the filter and search, oldest first.
    pub(super) visible: VecDeque<u64>,
    pub(super) filter: Filter,
    matcher: Option<AhoCorasick>,
    pub(super) search: Option<Entity<InputState>>,
    detail: Option<Detail>,
    /// The resolved URL of the current or last connection.
    url: SharedString,
    pub(super) scroll: UniformListScrollHandle,
    /// Measured at the interface size the list last used.
    row_height: Pixels,
    /// The list takes keyboard focus to move between messages.
    pub(super) focus: FocusHandle,
    split: Entity<ResizableState>,
    _search_subscription: Option<Subscription>,
}

impl MessageLog {
    pub(super) fn new(draft: WeakEntity<WebSocketDraft>, cx: &mut Context<Self>) -> Self {
        Self {
            draft,
            entries: VecDeque::new(),
            first: 0,
            bytes: 0,
            dropped: 0,
            visible: VecDeque::new(),
            filter: Filter::All,
            matcher: None,
            search: None,
            detail: None,
            url: SharedString::default(),
            scroll: UniformListScrollHandle::new(),
            row_height: ROW_HEIGHT.to_pixels(cx.theme().font_size),
            focus: cx.focus_handle().tab_stop(true),
            split: cx.new(|_| ResizableState::default()),
            _search_subscription: None,
        }
    }

    /// Create the search field once the log is shown in a window.
    pub(super) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search messages"));
        self._search_subscription = Some(cx.subscribe(
            &search,
            |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value();
                    this.set_query(&query, cx);
                }
            },
        ));
        self.search = Some(search);
    }

    /// What the draft's connection is doing.
    fn connection(&self, cx: &App) -> ConnectionState {
        self.draft
            .upgrade()
            .map_or(ConnectionState::Disconnected, |draft| {
                draft.read(cx).connection.state()
            })
    }

    /// The entry shown below the list.
    fn selected(&self) -> Option<u64> {
        self.detail.as_ref().map(|detail| detail.id)
    }

    /// Append events in the order they happened.
    pub(super) fn push(&mut self, events: Vec<WebSocketEvent>, cx: &mut Context<Self>) {
        let visible = self.visible.len();

        for event in events {
            let entry = self.entry(event);
            let id = self.first + self.entries.len() as u64;

            if self.matches(&entry) {
                self.visible.push_back(id);
            }
            self.bytes += entry.content.len();
            self.entries.push_back(entry);
        }

        let added = self.visible.len() - visible;

        while self.entries.len() > MAX_ENTRIES || (self.bytes > MAX_BYTES && self.entries.len() > 1)
        {
            let removed = self.entries.pop_front().unwrap();
            self.bytes -= removed.content.len();

            if self.visible.front() == Some(&self.first) {
                self.visible.pop_front();
            }
            if self.selected() == Some(self.first) {
                self.detail = None;
            }

            self.first += 1;
            self.dropped += 1;
        }

        self.keep_scroll_position(added);
        cx.notify();
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.first += self.entries.len() as u64;
        self.entries.clear();
        self.visible.clear();
        self.bytes = 0;
        self.dropped = 0;
        self.detail = None;
        cx.notify();
    }

    pub(super) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refresh_visible(cx);
    }

    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim();
        self.matcher = (!query.is_empty())
            .then(|| {
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build([query])
                    .ok()
            })
            .flatten();
        self.refresh_visible(cx);
    }

    fn refresh_visible(&mut self, cx: &mut Context<Self>) {
        self.visible = self
            .entries
            .iter()
            .zip(self.first..)
            .filter(|(entry, _)| self.matches(entry))
            .map(|(_, id)| id)
            .collect();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn matches(&self, entry: &Entry) -> bool {
        let kind = match self.filter {
            Filter::All => true,
            Filter::Sent => entry.kind == EntryKind::Sent,
            Filter::Received => entry.kind == EntryKind::Received,
        };

        kind && self.matcher.as_ref().is_none_or(|matcher| {
            matcher.is_match(entry.preview.as_ref())
                || matches!(&entry.content, Content::Text(text) if matcher.is_match(text.as_ref()))
        })
    }

    /// Rows are added above the viewport. While the newest rows are in view,
    /// new ones push the list down; otherwise the rows being read stay put.
    fn keep_scroll_position(&self, added: usize) {
        let state = self.scroll.0.borrow();
        let offset = state.base_handle.offset();

        if added > 0 && offset.y < px(0.) {
            state
                .base_handle
                .set_offset(point(offset.x, offset.y - self.row_height * added as f32));
        }
    }

    pub(super) fn entry_by_id(&self, id: u64) -> Option<&Entry> {
        id.checked_sub(self.first)
            .and_then(|index| self.entries.get(index as usize))
    }

    /// The entry shown in a row. Row zero is the newest.
    pub(super) fn row_entry(&self, row: usize) -> (u64, &Entry) {
        let id = self.visible[self.visible.len() - 1 - row];

        (id, &self.entries[(id - self.first) as usize])
    }

    /// Show an entry below the list, or hide it when it is already shown.
    pub(super) fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected() == Some(id) {
            self.detail = None;
            cx.notify();
            return;
        }

        let Some(entry) = self.entry_by_id(id) else {
            return;
        };
        let view: AnyView = match &entry.content {
            Content::Text(text) => match pretty_json(text) {
                Some(pretty) => {
                    let editor = cx.new(|cx| {
                        EditorState::new(window, cx)
                            .language("json")
                            .line_number(true)
                            .soft_wrap(true)
                            .searchable(true)
                            .replaceable(false)
                            .default_value(pretty)
                    });
                    cx.new(|_| ResponseBodyEditor(editor)).into()
                }
                None => cx.new(|cx| VirtualBody::new(text.clone(), true, cx)).into(),
            },
            Content::Binary(bytes) => cx
                .new(|cx| VirtualBody::new(hex_dump(bytes).into(), false, cx))
                .into(),
        };

        self.detail = Some(Detail { id, view });
        cx.notify();
    }

    /// The selected message's row, when the filter shows it.
    fn selected_row(&self) -> Option<usize> {
        self.selected()
            .and_then(|id| self.visible.iter().rposition(|&visible| visible == id))
            .map(|index| self.visible.len() - 1 - index)
    }

    /// Where the arrow keys act, which shows while the list has focus: the
    /// selected row while it is in view, otherwise the first row in view.
    fn keyboard_row(&self) -> Option<usize> {
        let last = self.visible.len().checked_sub(1)?;
        let state = self.scroll.0.borrow();
        let top = -state.base_handle.offset().y;
        // The list's last layout measured its viewport, not its rows.
        let bottom = state
            .last_item_size
            .map_or(Pixels::MAX, |size| top + size.item.height);
        // Rows that intersect the viewport, as the list renders them.
        let first = ((top / self.row_height).floor() as usize).min(last);
        let end = (bottom / self.row_height).ceil() as usize;

        match self.selected_row() {
            Some(row) if (first..end).contains(&row) => Some(row),
            // Prefer the first row that is fully in view.
            _ => Some(((top / self.row_height).ceil() as usize).min(last)),
        }
    }

    /// Up and Down show the neighboring message; Escape closes it. From a
    /// selection out of view, they show the first message in view.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.keyboard_row();
        let from_selection = row.is_some() && row == self.selected_row();
        let next = match (event.keystroke.key.as_str(), row) {
            ("down" | "up", Some(row)) if !from_selection => Some(row),
            ("down", Some(row)) => (row + 1 < self.visible.len()).then_some(row + 1),
            ("up", Some(row)) => row.checked_sub(1),
            ("escape", _) if self.detail.is_some() => {
                self.detail = None;
                cx.notify();
                cx.stop_propagation();
                return;
            }
            _ => return,
        };

        cx.stop_propagation();
        if let Some(row) = next {
            let (id, _) = self.row_entry(row);
            self.select(id, window, cx);
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    fn entry(&mut self, event: WebSocketEvent) -> Entry {
        let time = chrono::DateTime::<chrono::Local>::from(event.time)
            .format("%H:%M:%S%.3f")
            .to_string()
            .into();

        let (kind, content, size) = match event.kind {
            WebSocketEventKind::Sent(message) => {
                let size = message_size(&message);
                (EntryKind::Sent, message_content(message), Some(size))
            }
            WebSocketEventKind::Received(message) => {
                let size = message_size(&message);
                (EntryKind::Received, message_content(message), Some(size))
            }
            WebSocketEventKind::Connected(handshake) => {
                self.url = handshake.url.clone().into();
                (
                    EntryKind::Connected,
                    Content::Text(handshake_details(&handshake).into()),
                    None,
                )
            }
            WebSocketEventKind::Closed(close) => (
                EntryKind::Disconnected,
                Content::Text(close_details(&close).into()),
                None,
            ),
            WebSocketEventKind::NotSent(error) => (
                EntryKind::Error,
                Content::Text(format!("Message not sent: {error}").into()),
                None,
            ),
            WebSocketEventKind::Failed(error) => (
                EntryKind::Error,
                Content::Text(error.to_string().into()),
                None,
            ),
        };

        let preview = match (kind, &content) {
            (EntryKind::Connected, _) => format!("Connected to {}", self.url).into(),
            (EntryKind::Disconnected, Content::Text(details)) => {
                let summary = details.lines().nth(1).unwrap_or_default();
                format!("Disconnected from {} · {summary}", self.url).into()
            }
            (_, Content::Text(text)) => preview(text),
            (_, Content::Binary(_)) => "Binary message".into(),
        };

        Entry {
            kind,
            time,
            preview,
            size,
            content,
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (label, color) = match self.connection(cx) {
            ConnectionState::Disconnected => ("Disconnected", cx.theme().muted_foreground),
            ConnectionState::Connecting => ("Connecting…", cx.theme().info),
            ConnectionState::Connected => ("Connected", cx.theme().success),
            ConnectionState::Closing => ("Disconnecting…", cx.theme().warning),
        };
        let filter = self.filter;
        let view = cx.entity().downgrade();

        h_flex()
            .flex_none()
            .min_h_8()
            .gap_2()
            .child(
                h_flex()
                    .debug_selector(|| "websocket-status".into())
                    .flex_none()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius_tokens().md)
                    .bg(color.opacity(0.15))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(div().size_2().rounded(cx.theme().radius_full()).bg(color))
                    .child(label),
            )
            .when(self.dropped > 0, |row| {
                row.child(
                    div()
                        .debug_selector(|| "websocket-dropped".into())
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Earlier messages were removed"),
                )
            })
            .child(div().flex_1())
            .when_some(self.search.clone(), |row, search| {
                row.child(
                    div()
                        .debug_selector(|| "websocket-search".into())
                        .w(rems(14.))
                        .min_w_0()
                        .child(
                            Input::new(&search)
                                .small()
                                .prefix(IconName::Search)
                                .cleanable(true),
                        ),
                )
            })
            .child(
                Button::new("websocket-filter")
                    .debug_selector(|| "websocket-filter".into())
                    .ghost()
                    .small()
                    .label(filter.label())
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |mut menu, _, _| {
                        for option in [Filter::All, Filter::Sent, Filter::Received] {
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(option.label())
                                    .checked(option == filter)
                                    .on_click(move |_, _, cx| {
                                        let _ =
                                            view.update(cx, |log, cx| log.set_filter(option, cx));
                                    }),
                            );
                        }

                        menu
                    }),
            )
            .child(
                Button::new("clear-websocket-messages")
                    .debug_selector(|| "clear-websocket-messages".into())
                    .ghost()
                    .small()
                    .label("Clear")
                    .disabled(self.entries.is_empty())
                    .tooltip("Clear messages")
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            )
    }

    fn row(&self, row: usize, keyboard_row: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        let (id, entry) = self.row_entry(row);
        let selected = self.selected() == Some(id);
        // Where the arrow keys start: the selection, or else the newest row.
        let keyboard = keyboard_row == Some(row);
        let (icon, color) = kind_icon(entry.kind, cx);

        h_flex()
            .id(("websocket-message", id))
            .debug_selector(move || format!("websocket-message-{row}"))
            .h(ROW_HEIGHT)
            .w_full()
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius_tokens().md)
            .border_1()
            .border_color(transparent_black())
            .cursor_pointer()
            .when(selected, |this| {
                this.bg(cx.theme().list_active)
                    .border_color(cx.theme().list_active_border)
            })
            .when(!selected, |this| {
                this.hover(|this| this.bg(cx.theme().list_hover))
            })
            .when(keyboard, |this| this.border_color(cx.theme().ring))
            .child(
                Icon::new(icon)
                    .size(rems(0.875))
                    .flex_none()
                    .text_color(color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_sm()
                    .font_family(cx.theme().mono_font_family.clone())
                    .when(entry.kind == EntryKind::Error, |this| {
                        this.text_color(cx.theme().danger)
                    })
                    .child(entry.preview.clone()),
            )
            .when_some(entry.size, |this, size| {
                this.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(size_label(size)),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(entry.time.clone()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.select(id, window, cx);
            }))
            .into_any_element()
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("websocket-message-list")
            .debug_selector(|| "websocket-messages".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .size_full()
            .child(
                uniform_list(
                    "websocket-messages",
                    self.visible.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                        this.row_height = ROW_HEIGHT.to_pixels(window.rem_size());
                        let keyboard_row = this
                            .keyboard_row()
                            .filter(|_| this.focus.is_focused(window));
                        range.map(|row| this.row(row, keyboard_row, cx)).collect()
                    }),
                )
                .size_full()
                .track_scroll(&self.scroll),
            )
            .child(Scrollbar::vertical(&self.scroll))
    }

    fn detail(&self, detail: &Detail, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = detail.id;
        let entry = self.entry_by_id(id);
        let kind = entry.map_or(EntryKind::Received, |entry| entry.kind);

        v_flex()
            .debug_selector(|| "websocket-message-detail".into())
            .size_full()
            .min_h_0()
            .gap_1()
            .pt_1()
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(match kind {
                                EntryKind::Sent => "Sent",
                                EntryKind::Received => "Received",
                                EntryKind::Connected => "Connected",
                                EntryKind::Disconnected => "Disconnected",
                                EntryKind::Error => "Error",
                            }),
                    )
                    .when_some(entry, |row, entry| {
                        row.child(entry.time.clone())
                            .when_some(entry.size, |row, size| row.child(size_label(size)))
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("copy-websocket-message")
                            .debug_selector(|| "copy-websocket-message".into())
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .accessibility_label("Copy message")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // Build the text only when it is copied, not on every redraw.
                                if let Some(entry) = this.entry_by_id(id) {
                                    let text = match &entry.content {
                                        Content::Text(text) => text.to_string(),
                                        Content::Binary(bytes) => hex_dump(bytes),
                                    };
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                            })),
                    )
                    .child(
                        Button::new("close-websocket-message")
                            .debug_selector(|| "close-websocket-message".into())
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .accessibility_label("Close message")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detail = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded(cx.theme().radius_tokens().md)
                    .child(
                        detail
                            .view
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
            )
    }

    fn empty_state(&self, window: &Window, cx: &App) -> AnyElement {
        let media = EmptyMedia::new().with_variant(EmptyMediaVariant::Icon);
        let connection = self.connection(cx);
        let header = if connection == ConnectionState::Connecting {
            EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Loader).with_animation(
                    "websocket-connecting",
                    Animation::new(std::time::Duration::from_secs(1)).repeat(),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                )))
                .title(EmptyTitle::new().child("Connecting…"))
        } else if !self.entries.is_empty() {
            EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Search)))
                .title(EmptyTitle::new().child("No matching messages"))
        } else {
            EmptyHeader::new()
                .media(media.child(Icon::default().path("icons/arrow-up-down.svg")))
                .title(
                    EmptyTitle::new().child(if connection == ConnectionState::Connected {
                        "Waiting for messages"
                    } else {
                        "Connect to send and receive messages"
                    }),
                )
                .when_some(
                    Kbd::binding_for_action(&SendRequest, Some("Workspace"), window)
                        .filter(|_| connection == ConnectionState::Disconnected),
                    |header, kbd| {
                        header.description(
                            EmptyDescription::new().child(
                                h_flex()
                                    .justify_center()
                                    .gap_1()
                                    .child("Press")
                                    .child(kbd)
                                    .child("to connect"),
                            ),
                        )
                    },
                )
        };

        div()
            .debug_selector(|| "websocket-empty".into())
            .flex()
            .size_full()
            .child(Empty::new().header(header))
            .into_any_element()
    }
}

impl Render for MessageLog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.visible.is_empty() {
            self.empty_state(window, cx)
        } else if let Some(detail) = &self.detail {
            v_resizable("websocket-message-split")
                .with_state(&self.split)
                .child(resizable_panel().child(self.list(cx)))
                .child(
                    resizable_panel()
                        .size(rems(16.).to_pixels(window.rem_size()))
                        .size_range(rems(8.).to_pixels(window.rem_size())..Pixels::MAX)
                        .child(self.detail(detail, cx)),
                )
                .into_any_element()
        } else {
            self.list(cx).into_any_element()
        };

        v_flex()
            .debug_selector(|| "websocket-log".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.toolbar(cx))
            .child(div().flex_1().min_h_0().child(content))
    }
}

fn kind_icon(kind: EntryKind, cx: &App) -> (IconName, Hsla) {
    match kind {
        EntryKind::Sent => (IconName::ArrowUp, cx.theme().warning),
        EntryKind::Received => (IconName::ArrowDown, cx.theme().success),
        EntryKind::Connected => (IconName::CircleCheck, cx.theme().success),
        EntryKind::Disconnected => (IconName::CircleX, cx.theme().muted_foreground),
        EntryKind::Error => (IconName::TriangleAlert, cx.theme().danger),
    }
}

fn message_size(message: &WebSocketMessage) -> usize {
    match message {
        WebSocketMessage::Text(text) => text.len(),
        WebSocketMessage::Binary(bytes) => bytes.len(),
    }
}

fn message_content(message: WebSocketMessage) -> Content {
    match message {
        WebSocketMessage::Text(text) => Content::Text(text.into()),
        WebSocketMessage::Binary(bytes) => Content::Binary(bytes),
    }
}

/// The start of the text on one line.
fn preview(text: &str) -> SharedString {
    let mut preview = String::new();

    for (index, character) in text.trim().chars().enumerate() {
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
    if exceeds_editor_limit(text) || !text.trim_start().starts_with(['{', '[']) {
        return None;
    }

    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .filter(|pretty| !exceeds_editor_limit(pretty))
}

/// Offsets, hexadecimal bytes and printable ASCII, 16 bytes per line.
fn handshake_details(handshake: &WebSocketHandshake) -> String {
    let mut details = format!("GET {}\n\nRequest headers\n", handshake.url);

    for (name, value) in &handshake.request_headers {
        let _ = writeln!(details, "{name}: {value}");
    }

    let _ = write!(
        details,
        "\nResponse · {} · {} ms\n",
        handshake.status,
        handshake.elapsed.as_millis()
    );
    for (name, value) in &handshake.response_headers {
        let _ = writeln!(
            details,
            "{name}: {}",
            String::from_utf8_lossy(value.as_bytes())
        );
    }

    details
}

fn close_details(close: &WebSocketClose) -> String {
    let mut details = String::from(if close.by_client {
        "Closed by Request Eagle\n"
    } else {
        "Closed by the server\n"
    });

    match close.code {
        Some(code) => {
            let _ = write!(details, "{code} {}", close_code_name(code));
        }
        None => details.push_str("No status code"),
    }

    if !close.reason.is_empty() {
        let _ = write!(details, ": {}", close.reason);
    }

    details
}

fn close_code_name(code: u16) -> &'static str {
    match code {
        1000 => "Normal Closure",
        1001 => "Going Away",
        1002 => "Protocol Error",
        1003 => "Unsupported Data",
        1005 => "No Status Received",
        1006 => "Abnormal Closure",
        1007 => "Invalid Payload Data",
        1008 => "Policy Violation",
        1009 => "Message Too Big",
        1010 => "Mandatory Extension",
        1011 => "Internal Error",
        1012 => "Service Restart",
        1013 => "Try Again Later",
        1014 => "Bad Gateway",
        1015 => "TLS Handshake",
        _ => "",
    }
}

fn size_label(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}
