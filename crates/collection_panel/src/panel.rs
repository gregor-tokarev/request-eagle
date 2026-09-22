use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::{CollectionRegistry, MovePlacement, Request};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{editing::RenameEditor, tree::CollectionTree};

pub enum CollectionPanelEvent {
    CollectionRelocated {
        previous_path: PathBuf,
        path: PathBuf,
        environment_path: PathBuf,
    },
    RequestRelocated {
        id: SharedString,
        previous_path: PathBuf,
        path: PathBuf,
        environment_path: PathBuf,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
    },
    OpenRequest {
        id: SharedString,
        path: PathBuf,
        environment_path: PathBuf,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
        request: Request,
    },
}

/// The collections tree and its search, editing, and drag interactions.
pub struct CollectionPanel {
    pub(super) collections: CollectionRegistry,
    pub(super) rename: Option<RenameEditor>,
    pub(super) pending_delete: Option<PathBuf>,
    pub(super) drop_target: Option<(usize, MovePlacement)>,
    pub(super) error: Option<String>,
    pub(super) tree: Arc<CollectionTree>,
    pub(super) visible: Arc<Vec<usize>>,
    pub(super) unfiltered_rows: Option<Arc<Vec<usize>>>,
    pub(super) collapsed: HashSet<usize>,
    pub(super) selected: Option<usize>,
    pub(super) selected_row: Option<usize>,
    pub(super) search: Entity<InputState>,
    pub(super) query: String,
    pub(super) scroll_handle: UniformListScrollHandle,
    pub(super) focus: FocusHandle,
    pub(super) delete_focus: FocusHandle,
    pub(super) rows_task: Option<Task<()>>,
    _search_subscription: Subscription,
    _focus_subscription: Subscription,
}

impl EventEmitter<CollectionPanelEvent> for CollectionPanel {}

impl CollectionPanel {
    pub fn new(
        collections: CollectionRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree = Arc::new(CollectionTree::new(&collections));
        let visible: Arc<Vec<usize>> = Arc::new((0..tree.items.len()).collect());
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter collections"));
        let search_subscription = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Focus | InputEvent::Change) {
                this.pending_delete = None;
                cx.notify();
            }

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
            collections,
            rename: None,
            pending_delete: None,
            drop_target: None,
            error: None,
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
            delete_focus: cx.focus_handle(),
            rows_task: None,
            _search_subscription: search_subscription,
            _focus_subscription: focus_subscription,
        }
    }

    /// Save the editor's request and refresh the snapshot used when reopening it.
    pub fn save_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Result<(), collection::CollectionEditError> {
        self.collections
            .update_request(path, expected_id, request)?;
        let file = self.collections.file(path).expect("saved request exists");

        if self.tree.request_changed(file) {
            // Cancel any result computed from the previous search documents.
            self.rows_task = None;
            Arc::make_mut(&mut self.tree).update_request(file);
            self.refresh_rows(false, cx);
        }

        Ok(())
    }

    pub(super) fn refresh_rows(&mut self, reset_scroll: bool, cx: &mut Context<Self>) {
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

    pub(super) fn apply_rows(
        &mut self,
        rows: Arc<Vec<usize>>,
        reset_scroll: bool,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.query.is_empty() || !self.tree.items[index].is_branch() {
            return;
        }

        if !self.collapsed.remove(&index) {
            self.collapsed.insert(index);
        }

        self.unfiltered_rows = None;
        self.refresh_rows(false, cx);
    }

    pub(super) fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(&index) = self.visible.get(row) {
            if self.selected != Some(index) {
                self.pending_delete = None;
            }

            self.selected = Some(index);
            self.selected_row = Some(row);
            self.scroll_handle
                .scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !(self.focus.is_focused(window) || self.delete_focus.is_focused(window))
            || self.visible.is_empty()
            || event.keystroke.modifiers != Modifiers::default()
        {
            return;
        }

        let row = self.selected_row.unwrap_or(0);
        let index = self.visible[row];

        if self.delete_focus.is_focused(window)
            && matches!(event.keystroke.key.as_str(), "up" | "down" | "home" | "end")
        {
            self.pending_delete = None;
            window.focus(&self.focus, cx);
        }

        match event.keystroke.key.as_str() {
            "down" => self.select_row(
                self.selected_row
                    .map_or(0, |row| (row + 1).min(self.visible.len() - 1)),
                cx,
            ),
            "up" => self.select_row(row.saturating_sub(1), cx),
            "home" => self.select_row(0, cx),
            "end" => self.select_row(self.visible.len() - 1, cx),
            "backspace" => {
                if self.selected_row.is_some() {
                    self.request_delete(index, window, cx);
                }
            }
            "enter" => self.begin_rename(index, window, cx),
            "space" => self.toggle(index, cx),
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
}

impl Focusable for CollectionPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CollectionPanel {
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
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("new-collection")
                            .debug_selector(|| "new-collection".into())
                            .icon(IconName::Plus)
                            .tooltip("New Collection")
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_collection(window, cx)
                            })),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                div()
                    .id("sidebar-tree")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .capture_key_down(cx.listener(Self::on_delete_key_down))
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
