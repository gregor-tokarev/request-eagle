use std::{collections::HashSet, sync::Arc};

use collection::CollectionRegistry;
use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::tree::{CollectionTree, ItemKind};

pub(crate) struct Sidebar {
    pub(super) tree: Arc<CollectionTree>,
    pub(super) visible: Arc<Vec<usize>>,
    unfiltered_rows: Option<Arc<Vec<usize>>>,
    pub(super) collapsed: HashSet<usize>,
    pub(super) selected: Option<usize>,
    selected_row: Option<usize>,
    pub(super) search: Entity<InputState>,
    query: String,
    scroll_handle: UniformListScrollHandle,
    focus: FocusHandle,
    rows_task: Option<Task<()>>,
    _search_subscription: Subscription,
    _focus_subscription: Subscription,
}

impl Sidebar {
    pub(crate) fn new(
        collections: Arc<CollectionRegistry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree = Arc::new(CollectionTree::new(&collections));
        let visible: Arc<Vec<usize>> = Arc::new((0..tree.items.len()).collect());
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter collections"));
        let search_subscription = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = this.search.read(cx).value().trim().to_lowercase();
                this.refresh_rows(true, cx);
            }
        });

        let focus = cx.focus_handle().tab_stop(true);
        let focus_subscription = cx.on_focus(&focus, window, |this, _, cx| {
            this.select_row(this.selected_row.unwrap_or(0), cx);
        });

        Self {
            tree,
            unfiltered_rows: Some(visible.clone()),
            visible,
            collapsed: HashSet::new(),
            selected: None,
            selected_row: None,
            search,
            query: String::new(),
            scroll_handle: UniformListScrollHandle::new(),
            focus,
            rows_task: None,
            _search_subscription: search_subscription,
            _focus_subscription: focus_subscription,
        }
    }

    fn refresh_rows(&mut self, reset_scroll: bool, cx: &mut Context<Self>) {
        self.rows_task = None;

        if self.query.is_empty()
            && let Some(rows) = self.unfiltered_rows.clone()
        {
            self.apply_rows(rows, reset_scroll, cx);
            return;
        }

        let tree = self.tree.clone();
        let collapsed = self.collapsed.clone();
        let query = self.query.clone();
        let task = cx
            .background_executor()
            .spawn(async move { Arc::new(tree.visible_rows(&collapsed, &query)) });

        // Dropping the previous task prevents an older search replacing newer results.
        self.rows_task = Some(cx.spawn(async move |this, cx| {
            let rows = task.await;

            let _ = this.update(cx, |this, cx| {
                if this.query.is_empty() {
                    this.unfiltered_rows = Some(rows.clone());
                }

                this.apply_rows(rows, reset_scroll, cx);
            });
        }));

        cx.notify();
    }

    fn apply_rows(&mut self, rows: Arc<Vec<usize>>, reset_scroll: bool, cx: &mut Context<Self>) {
        self.visible = rows;
        self.selected_row = self
            .selected
            .and_then(|selected| self.visible.binary_search(&selected).ok());

        if reset_scroll {
            self.scroll_handle
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }

        cx.notify();
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.query.is_empty() || !self.tree.items[index].is_branch() {
            return;
        }

        if !self.collapsed.remove(&index) {
            self.collapsed.insert(index);
        }

        self.unfiltered_rows = None;
        self.refresh_rows(false, cx);
    }

    fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(&index) = self.visible.get(row) {
            self.selected = Some(index);
            self.selected_row = Some(row);
            self.scroll_handle
                .scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.visible.is_empty() || event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        let row = self.selected_row.unwrap_or(0);
        let index = self.visible[row];

        match event.keystroke.key.as_str() {
            "down" => self.select_row(
                self.selected_row
                    .map_or(0, |row| (row + 1).min(self.visible.len() - 1)),
                cx,
            ),
            "up" => self.select_row(row.saturating_sub(1), cx),
            "home" => self.select_row(0, cx),
            "end" => self.select_row(self.visible.len() - 1, cx),
            "enter" | "space" => self.toggle(index, cx),
            "right" => {
                if self.collapsed.contains(&index) {
                    self.toggle(index, cx);
                } else if self.tree.items[index].is_branch()
                    && row + 1 < self.visible.len()
                    && self.visible[row + 1] < self.tree.items[index].end
                {
                    self.select_row(row + 1, cx);
                }
            }
            "left" => {
                if self.tree.items[index].is_branch()
                    && !self.collapsed.contains(&index)
                    && self.query.is_empty()
                {
                    self.toggle(index, cx);
                } else if let Some(parent) = self.tree.items[index].parent
                    && let Ok(row) = self.visible[..row].binary_search(&parent)
                {
                    self.select_row(row, cx);
                }
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
        if self.visible.is_empty() || event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        let row = match event.keystroke.key.as_str() {
            "down" | "enter" => 0,
            "up" => self.visible.len() - 1,
            _ => return,
        };

        self.select_row(row, cx);
        window.focus(&self.focus, cx);
        cx.stop_propagation();
    }

    fn row(&self, row: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.visible[row];
        let item = &self.tree.items[index];
        let branch = item.is_branch();
        let expanded = !self.query.is_empty() || !self.collapsed.contains(&index);
        let selected = self.selected == Some(index);
        let theme = cx.theme();

        div()
            .id(("collection-row", index))
            .debug_selector(move || format!("collection-row-{index}").into())
            .h(px(30.))
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .size_full()
                    .rounded_md()
                    .pl(px(6. + item.depth as f32 * 14.))
                    .pr_2()
                    .gap_1p5()
                    .text_size(px(13.))
                    .when(selected, |this| this.bg(theme.sidebar_accent))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
                    })
                    .child(if branch {
                        h_flex()
                            .gap_1p5()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(12.)),
                            )
                            .child(Icon::new(IconName::Folder).size(px(14.)))
                            .into_any_element()
                    } else {
                        let ItemKind::Request(method) = item.kind else {
                            unreachable!()
                        };
                        let color = match method {
                            "GET" => theme.success,
                            "POST" => theme.warning,
                            "PUT" => theme.info,
                            _ => theme.danger,
                        };

                        div()
                            .w(px(42.))
                            .flex_none()
                            .text_size(px(9.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(method)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .when(item.kind == ItemKind::Collection, |this| {
                                this.font_weight(FontWeight::MEDIUM)
                            })
                            .child(item.label.clone()),
                    )
                    .when(branch, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(theme.muted_foreground)
                                .child(item.request_count.to_string()),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.select_row(row, cx);
                this.toggle(index, cx);
            }))
    }
}

impl Focusable for Sidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "collections-sidebar".into())
            .size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                div()
                    .flex_none()
                    .p_2()
                    // Input actions consume arrow keys, so transfer focus in capture phase.
                    .capture_key_down(cx.listener(Self::on_search_key_down))
                    .child(
                        Input::new(&self.search)
                            .small()
                            .prefix(IconName::Search)
                            .cleanable(true),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h(px(30.))
                    .px_4()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("COLLECTIONS"),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(cx.theme().muted_foreground)
                            .child(self.tree.roots.len().to_string()),
                    ),
            )
            .child(
                div()
                    .id("sidebar-tree")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(if self.visible.is_empty() {
                        v_flex()
                            .p_4()
                            .gap_1()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.tree.items.is_empty() {
                                "No collections yet"
                            } else {
                                "No matching requests"
                            })
                            .child(if self.tree.items.is_empty() {
                                "Your collections will appear here."
                            } else {
                                "Try a name, method, or URL."
                            })
                            .into_any_element()
                    } else {
                        uniform_list(
                            "sidebar-scroll",
                            self.visible.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|row| this.row(row, cx).into_any_element())
                                    .collect()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.scroll_handle)
                        .into_any_element()
                    })
                    .when(!self.visible.is_empty(), |this| {
                        this.child(Scrollbar::vertical(&self.scroll_handle))
                    }),
            )
    }
}
