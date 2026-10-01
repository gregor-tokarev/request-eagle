use chrono::{DateTime, Datelike as _, Local, NaiveDate};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenuItem},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_color;
use request_history::{Entry, History, Record};
use tab_ui::RequestSent;

pub(crate) enum HistoryPanelEvent {
    Open { entry: Entry, record: Record },
}

enum Row {
    Day(SharedString),
    /// An index into the history's entries.
    Entry(usize),
}

/// The sidebar section that lists sent requests by day, newest first.
pub(crate) struct HistoryPanel {
    history: History,
    rows: Vec<Row>,
    query: String,
    search: Entity<InputState>,
    /// The selected entry's id, which stays selected as requests arrive.
    selected: Option<String>,
    pending_clear: bool,
    error: Option<String>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _search_subscription: Subscription,
}

impl EventEmitter<HistoryPanelEvent> for HistoryPanel {}

impl HistoryPanel {
    pub(crate) fn new(mut history: History, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let error = history
            .load()
            .err()
            .map(|error| format!("Could not read history: {error}"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter history"));
        let search_subscription = cx.subscribe(&search, |this, search, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().trim().to_lowercase();
                this.refresh(cx);
                this.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
            }
        });

        let mut panel = Self {
            history,
            rows: Vec::new(),
            query: String::new(),
            search,
            selected: None,
            pending_clear: false,
            error,
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            _search_subscription: search_subscription,
        };
        panel.refresh(cx);

        panel
    }

    /// Whether focus is in the filter or the list.
    pub(crate) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx) || self.search.focus_handle(cx).is_focused(window)
    }

    pub(crate) fn count(&self) -> usize {
        self.history.entries().len()
    }

    pub(crate) fn record(&mut self, sent: &RequestSent, cx: &mut Context<Self>) {
        self.error = self
            .history
            .add(&sent.record, sent.sent_at)
            .err()
            .map(|error| format!("Could not save history: {error}"));
        self.refresh(cx);
    }

    /// Ask to delete every entry. The section shows a confirmation first.
    pub(crate) fn request_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.count() == 0 {
            return;
        }

        self.pending_clear = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_clear = false;
        self.selected = None;
        self.error = self
            .history
            .clear()
            .err()
            .map(|error| format!("Could not clear history: {error}"));
        window.focus(&self.focus, cx);
        self.refresh(cx);
    }

    fn cancel_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_clear = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn delete(&mut self, id: &str, cx: &mut Context<Self>) {
        // Keep a selection in the same place, so the keyboard can delete
        // several entries in a row.
        if self.selected.as_deref() == Some(id) {
            let rows = self.entry_rows();
            let position = rows.iter().position(|&index| self.entry(index).id == id);
            self.selected = position
                .and_then(|position| {
                    rows.get(position + 1)
                        .or_else(|| rows.get(position.checked_sub(1)?))
                })
                .map(|&index| self.entry(index).id.clone());
        }

        self.error = self
            .history
            .delete(id)
            .err()
            .map(|error| format!("Could not delete from history: {error}"));
        self.refresh(cx);
    }

    fn open(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());

        let Some(entry) = self
            .history
            .entries()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return;
        };

        match self.history.read(id) {
            Ok(record) => {
                self.error = None;
                cx.emit(HistoryPanelEvent::Open { entry, record });
            }
            Err(error) => self.error = Some(format!("Could not open from history: {error}")),
        }

        cx.notify();
    }

    fn entry(&self, index: usize) -> &Entry {
        &self.history.entries()[index]
    }

    /// The entries shown, as indices into the history's entries.
    fn entry_rows(&self) -> Vec<usize> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Entry(index) => Some(*index),
                Row::Day(_) => None,
            })
            .collect()
    }

    /// List the entries that match the filter under the day they were sent.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let today = Local::now().date_naive();
        let mut day = None;
        self.rows.clear();

        for (index, entry) in self.history.entries().iter().enumerate() {
            if !self.query.is_empty()
                && !entry.address.to_lowercase().contains(&self.query)
                && !entry.label.to_lowercase().contains(&self.query)
            {
                continue;
            }

            let date = DateTime::<Local>::from(entry.sent_at()).date_naive();
            if day != Some(date) {
                day = Some(date);
                self.rows.push(Row::Day(day_label(date, today).into()));
            }

            self.rows.push(Row::Entry(index));
        }

        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.history.entries().iter().any(|entry| &entry.id == id))
        {
            self.selected = None;
        }

        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = Some(self.entry(index).id.clone());

        if let Some(row) = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Entry(entry) if *entry == index))
        {
            // Show the day above the first entry of the list.
            let row = if row == 1 { 0 } else { row };
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }

        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        if self.pending_clear {
            match event.keystroke.key.as_str() {
                "enter" => self.clear(window, cx),
                "escape" => self.cancel_clear(window, cx),
                _ => return,
            }

            cx.stop_propagation();
            return;
        }

        let rows = self.entry_rows();
        if rows.is_empty() {
            return;
        }

        let position = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|&index| &self.entry(index).id == id));
        let current = position.unwrap_or(0);

        match event.keystroke.key.as_str() {
            "down" => self.select(
                rows[position.map_or(0, |row| (row + 1).min(rows.len() - 1))],
                cx,
            ),
            "up" => self.select(rows[current.saturating_sub(1)], cx),
            "home" => self.select(rows[0], cx),
            "end" => self.select(rows[rows.len() - 1], cx),
            "enter" => {
                let id = self.entry(rows[current]).id.clone();
                self.open(&id, cx);
            }
            "backspace" | "delete" if position.is_some() => {
                let id = self.entry(rows[current]).id.clone();
                self.delete(&id, cx);
            }
            _ => return,
        }

        cx.stop_propagation();
    }

    fn on_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        if event.keystroke.key == "escape" {
            self.search
                .update(cx, |search, cx| search.clean(window, cx));
            window.focus(&self.focus, cx);
            cx.stop_propagation();
            return;
        }

        let rows = self.entry_rows();
        let index = match event.keystroke.key.as_str() {
            "down" | "enter" => rows.first(),
            "up" => rows.last(),
            _ => return,
        };

        if let Some(&index) = index {
            self.select(index, cx);
            window.focus(&self.focus, cx);
            cx.stop_propagation();
        }
    }

    fn clear_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div().flex_none().h_8().w_full().px_2().child(
            h_flex()
                .debug_selector(|| "history-clear-prompt".into())
                .size_full()
                .rounded(cx.theme().radius_tokens().md)
                .px_2()
                .gap_1()
                .bg(cx.theme().sidebar_accent)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .child("Clear all history?"),
                )
                .child(
                    Button::new("confirm-history-clear")
                        .debug_selector(|| "confirm-history-clear".into())
                        .label("Clear")
                        .tooltip("Delete every request in history (Enter)")
                        .xsmall()
                        .danger()
                        .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                )
                .child(
                    Button::new("cancel-history-clear")
                        .debug_selector(|| "cancel-history-clear".into())
                        .label("Cancel")
                        .tooltip("Keep history (Escape)")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| this.cancel_clear(window, cx))),
                ),
        )
    }

    fn row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = match &self.rows[row] {
            Row::Day(label) => {
                return div()
                    .debug_selector(move || format!("history-day-{row}"))
                    .h_8()
                    .w_full()
                    .px_4()
                    .flex()
                    .items_end()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(label.clone())
                    .into_any_element();
            }
            Row::Entry(index) => *index,
        };

        let theme = cx.theme();
        let entry = self.entry(index);
        let id = entry.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let label: SharedString = entry.label.clone().into();
        let address: SharedString = match short_address(&entry.address) {
            "" => "Untitled".into(),
            address => address.to_owned().into(),
        };
        let time = DateTime::<Local>::from(entry.sent_at())
            .format("%H:%M")
            .to_string();
        let view = cx.entity().downgrade();
        let focus = self.focus.clone();

        div()
            .id(ElementId::Name(id.clone().into()))
            .debug_selector(move || format!("history-row-{row}"))
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .size_full()
                    .rounded(theme.radius_tokens().md)
                    .pl_2()
                    .pr_2()
                    .gap_2()
                    .text_sm()
                    .when(selected, |this| {
                        this.bg(theme.tokens.sidebar_accent.background)
                            .text_color(theme.sidebar_accent_foreground)
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(method_color(&label, cx))
                            .child(label),
                    )
                    .child(div().flex_1().min_w_0().text_ellipsis().child(address))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(time),
                    ),
            )
            .on_click(cx.listener({
                let id = id.clone();
                move |this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    this.open(&id, cx);
                }
            }))
            .capture_any_mouse_down(cx.listener({
                let id = id.clone();
                move |this, event: &MouseDownEvent, window, cx| {
                    if event.button == MouseButton::Right {
                        window.focus(&this.focus, cx);
                        this.selected = Some(id.clone());
                        cx.notify();
                    }
                }
            }))
            .context_menu(move |menu, _, _| {
                let open_view = view.clone();
                let delete_view = view.clone();
                let open_id = id.clone();
                let delete_id = id.clone();

                menu.action_context(focus.clone())
                    .item(PopupMenuItem::new("Open").on_click(move |_, _, cx| {
                        let _ = open_view.update(cx, |this, cx| this.open(&open_id, cx));
                    }))
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete from History").on_click(move |_, _, cx| {
                            let _ = delete_view.update(cx, |this, cx| this.delete(&delete_id, cx));
                        }),
                    )
            })
            .into_any_element()
    }
}

/// The address without its scheme, which leaves room for the path.
pub(crate) fn short_address(address: &str) -> &str {
    address
        .split_once("://")
        .filter(|(scheme, _)| !scheme.contains(['/', '{']))
        .map_or(address, |(_, rest)| rest)
}

/// How the history names the day a request was sent.
pub(crate) fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".into()
    } else if today.pred_opt() == Some(date) {
        "Yesterday".into()
    } else if date.year() == today.year() {
        date.format("%B %-d").to_string()
    } else {
        date.format("%B %-d, %Y").to_string()
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.rows.is_empty();

        v_flex()
            .debug_selector(|| "history-sidebar".into())
            .size_full()
            .child(
                div()
                    .debug_selector(|| "history-search".into())
                    .flex_none()
                    .px_2()
                    .pb_2()
                    .capture_key_down(cx.listener(Self::on_search_key_down))
                    .child(
                        Input::new(&self.search)
                            .small()
                            .prefix(IconName::Search)
                            .cleanable(true),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when(self.pending_clear, |this| this.child(self.clear_prompt(cx)))
            .child(
                div()
                    .id("history-list")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(if empty {
                        v_flex()
                            .p_4()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.count() == 0 {
                                "No requests yet"
                            } else {
                                "No matching requests"
                            })
                            .child(if self.count() == 0 {
                                "Requests you send appear here, with their responses."
                            } else {
                                "Try a method or URL."
                            })
                            .into_any_element()
                    } else {
                        uniform_list(
                            "history-scroll",
                            self.rows.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|row| this.row(row, cx)).collect()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.scroll)
                        .into_any_element()
                    })
                    .when(!empty, |this| this.child(Scrollbar::vertical(&self.scroll))),
            )
    }
}
