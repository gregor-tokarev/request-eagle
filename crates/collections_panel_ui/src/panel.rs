use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::{Collections, CollectionsEvent, MovePlacement};

use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::HttpRequest;

use super::{
    actions::{DeleteItem, RenameItem},
    editing::RenameEditor,
    tree::{CollectionTree, ItemKind},
};

/// What the user asked of the sidebar. Changes to collections themselves are
/// `CollectionsEvent`s.
// Events are passed on one at a time, so boxing the request would not help.
#[allow(clippy::large_enum_variant)]
pub enum CollectionPanelEvent {
    /// Open the collection or request at the path in a tab.
    Open(PathBuf),
    /// Open the Collection Runner for the collection's or folder's requests.
    Run(PathBuf),
    /// A request imported without saving it, such as a pasted cURL command.
    OpenUnsavedRequest(HttpRequest),
    /// Imported environments were added to the environments directory.
    EnvironmentsImported,
}

/// The collections tree and its search, editing, and drag interactions.
pub struct CollectionPanel {
    pub(super) collections: Entity<Collections>,
    /// The revision of the collections that the tree shows.
    pub(super) revision: u64,
    pub(super) rename: Option<RenameEditor>,
    pub(super) pending_delete: Option<PathBuf>,
    /// The row whose "…" menu is open, which keeps its button shown.
    pub(super) menu_row: Option<PathBuf>,
    pub(super) drop_target: Option<(usize, MovePlacement)>,
    pub(super) error: Option<String>,
    pub(super) tree: Arc<CollectionTree>,
    pub(super) visible: Arc<Vec<usize>>,
    pub(super) unfiltered_rows: Option<Arc<Vec<usize>>>,
    pub(super) collapsed: HashSet<usize>,
    pub(super) selected: Option<usize>,
    /// The branch the last click expanded or collapsed. That can scroll
    /// another row under the pointer, so quick follow-up presses toggle this
    /// branch again unless the list is scrolled in between.
    pub(super) clicked_branch: Option<PathBuf>,
    pub(super) search: Entity<InputState>,
    pub(super) query: String,
    pub(super) scroll_handle: UniformListScrollHandle,
    pub(super) focus: FocusHandle,
    pub(super) delete_focus: FocusHandle,
    pub(super) rows_task: Option<Task<()>>,
    _search_subscription: Subscription,
    _focus_subscription: Subscription,
    _collections_subscription: Subscription,
}

impl EventEmitter<CollectionPanelEvent> for CollectionPanel {}

impl CollectionPanel {
    /// `collapsed` holds the paths of the collections and folders that start
    /// collapsed, as `collapsed_paths` gave them in the last session.
    pub fn new(
        collections: Entity<Collections>,
        collapsed: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tree, revision) = {
            let collections = collections.read(cx);
            (
                Arc::new(CollectionTree::new(collections.registry())),
                collections.revision(),
            )
        };
        let collapsed = tree.branches_at(&collapsed.into_iter().collect());
        let visible = Arc::new(tree.visible_rows(&collapsed, ""));
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
            this.select_row(this.selected_row().unwrap_or(0), cx);
        });
        let collections_subscription =
            cx.subscribe_in(&collections, window, Self::on_collections_event);

        Self {
            collections,
            revision,
            rename: None,
            pending_delete: None,
            menu_row: None,
            drop_target: None,
            error: None,
            tree,
            unfiltered_rows: Some(visible.clone()),
            visible,
            collapsed,
            selected: None,
            clicked_branch: None,
            search,
            query: String::new(),
            scroll_handle: UniformListScrollHandle::new(),
            focus,
            delete_focus: cx.focus_handle(),
            rows_task: None,
            _search_subscription: search_subscription,
            _focus_subscription: focus_subscription,
            _collections_subscription: collections_subscription,
        }
    }

    /// Follow changes made elsewhere, such as a request saved or renamed in
    /// its tab. The tree already shows the changes made from it.
    fn on_collections_event(
        &mut self,
        _: &Entity<Collections>,
        event: &CollectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let revision = self.collections.read(cx).revision();
        if revision == self.revision {
            return;
        }

        match event {
            // Saving a request changes at most its own row.
            CollectionsEvent::RequestSaved(path) if revision == self.revision + 1 => {
                self.revision = revision;
                self.update_request_row(path, cx);
            }
            CollectionsEvent::Created(path) => self.reveal(path, None, window, cx),
            CollectionsEvent::CollectionRenamed {
                previous_path,
                path,
            }
            | CollectionsEvent::FolderRelocated {
                previous_path,
                path,
                ..
            } => {
                let selected = self.selected.map(|index| {
                    let selected = &self.tree.items[index].path;
                    match selected.strip_prefix(previous_path) {
                        Ok(relative) => path.join(relative),
                        Err(_) => selected.clone(),
                    }
                });
                self.rebuild_tree(selected.as_deref(), Some((previous_path, path)), cx);
            }
            CollectionsEvent::RequestSaved(_)
            | CollectionsEvent::RequestRelocated { .. }
            | CollectionsEvent::Deleted(_) => {
                let selected = self
                    .selected
                    .map(|index| self.tree.items[index].path.clone());
                self.rebuild_tree(selected.as_deref(), None, cx);
            }
        }
    }

    fn update_request_row(&mut self, path: &Path, cx: &mut Context<Self>) {
        let collections = self.collections.read(cx);
        let Some(file) = collections.registry().file(path) else {
            return;
        };
        if !self.tree.request_changed(file) {
            return;
        }

        // Cancel any result computed from the previous search documents.
        self.rows_task = None;
        Arc::make_mut(&mut self.tree).update_request(file);
        self.refresh_rows(false, cx);
    }

    pub fn collection_count(&self) -> usize {
        self.tree.roots.len()
    }

    /// The collapsed collections and folders in tree order, to restore them
    /// in the next session.
    pub fn collapsed_paths(&self) -> Vec<PathBuf> {
        let mut collapsed: Vec<_> = self.collapsed.iter().copied().collect();
        collapsed.sort_unstable();

        collapsed
            .into_iter()
            .map(|index| self.tree.items[index].path.clone())
            .collect()
    }

    /// Whether focus is in the search field or the tree.
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx) || self.search.focus_handle(cx).is_focused(window)
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            search.select_all(window, cx);
            search.focus(window, cx);
        });
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

        if reset_scroll {
            self.scroll_handle
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }

        cx.notify();
    }

    /// The selected item's row, while the item is visible.
    pub(super) fn selected_row(&self) -> Option<usize> {
        self.selected
            .and_then(|selected| self.visible.binary_search(&selected).ok())
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
            self.scroll_handle
                .scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Open a collection or request row in a tab; folder rows have no page.
    pub(super) fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = &self.tree.items[index];

        if matches!(item.kind, ItemKind::Collection | ItemKind::Request(_)) {
            cx.emit(CollectionPanelEvent::Open(item.path.clone()));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !(self.focus.is_focused(window) || self.delete_focus.is_focused(window))
            || self.visible.is_empty()
            || event.keystroke.modifiers != Modifiers::default()
        {
            return;
        }

        let row = self.selected_row().unwrap_or(0);
        let index = self.visible[row];

        if self.delete_focus.is_focused(window)
            && matches!(event.keystroke.key.as_str(), "up" | "down" | "home" | "end")
        {
            self.pending_delete = None;
            window.focus(&self.focus, cx);
        }

        match event.keystroke.key.as_str() {
            "down" => {
                let next = match self.selected_row() {
                    Some(row) => self.item_row(row + 1, true),
                    None => Some(0),
                };
                if let Some(row) = next {
                    self.select_row(row, cx);
                }
            }
            "up" => {
                if let Some(row) = row.checked_sub(1).and_then(|row| self.item_row(row, false)) {
                    self.select_row(row, cx);
                }
            }
            "home" => self.select_row(0, cx),
            "end" => {
                if let Some(row) = self.item_row(self.visible.len() - 1, false) {
                    self.select_row(row, cx);
                }
            }
            "enter" => self.open(index, cx),
            "space" => self.toggle(index, cx),
            "right" => {
                if self.collapsed.contains(&index) {
                    self.toggle(index, cx);
                } else if self.tree.items[index].is_branch()
                    && row + 1 < self.visible.len()
                    && self.visible[row + 1] < self.tree.items[index].end
                    && self.tree.items[self.visible[row + 1]].kind != ItemKind::Empty
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

    /// The nearest row from `row` on, forward or back, that holds an item.
    /// The placeholders of empty branches only show a message, so keys
    /// skip them.
    fn item_row(&self, row: usize, forward: bool) -> Option<usize> {
        let holds_item = |row: &usize| self.tree.items[self.visible[*row]].kind != ItemKind::Empty;

        if forward {
            (row..self.visible.len()).find(holds_item)
        } else {
            (0..=row.min(self.visible.len() - 1)).rev().find(holds_item)
        }
    }

    fn rename_selected(&mut self, _: &RenameItem, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&index) = self.visible.get(self.selected_row().unwrap_or(0)) {
            self.begin_rename(index, window, cx);
        }
    }

    fn delete_selected(&mut self, _: &DeleteItem, window: &mut Window, cx: &mut Context<Self>) {
        // Repeating the shortcut in an open prompt must not confirm it.
        if self.pending_delete.is_some() {
            return;
        }

        if let Some(&index) = self.selected_row().and_then(|row| self.visible.get(row)) {
            self.request_delete(index, window, cx);
        }
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

        if self.visible.is_empty() {
            return;
        }

        let row = match event.keystroke.key.as_str() {
            "down" | "enter" => 0,
            "up" => match self.item_row(self.visible.len() - 1, false) {
                Some(row) => row,
                None => return,
            },
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
        let skipped = self.collections.read(cx).registry().skipped().len();

        v_flex()
            .debug_selector(|| "collections-sidebar".into())
            .size_full()
            .child(
                div()
                    .debug_selector(|| "collections-search".into())
                    .flex_none()
                    .px_2()
                    .pb_2()
                    // Input actions consume these keys, so transfer focus in capture phase.
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
            // Only the count: request-eagle-cli tells agents which files and why.
            .when(skipped > 0, |this| {
                this.child(
                    h_flex()
                        .debug_selector(|| "collections-skipped".into())
                        .px_3()
                        .pb_2()
                        .gap_1()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::TriangleAlert).xsmall())
                        .child(match skipped {
                            1 => "Couldn't load 1 file".to_owned(),
                            count => format!("Couldn't load {count} files"),
                        }),
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
                    .key_context("CollectionsSidebar")
                    .on_action(cx.listener(Self::rename_selected))
                    .on_action(cx.listener(Self::delete_selected))
                    .capture_key_down(cx.listener(Self::on_delete_key_down))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, _| {
                        if event.click_count == 1 {
                            this.clicked_branch = None;
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
                        this.clicked_branch = None;
                    }))
                    .child(if self.visible.is_empty() {
                        v_flex()
                            .p_4()
                            .gap_1()
                            .text_xs()
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
