use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use collection::{CollectionRegistry, MovePlacement};

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Request, RequestScripts};

use super::{
    actions::{DeleteItem, RenameItem},
    editing::RenameEditor,
    tree::{CollectionTree, ItemKind},
};

/// How long a clicked collection waits for a second click before it
/// expands or collapses.
pub(super) const DOUBLE_CLICK_WAIT: Duration = Duration::from_millis(250);

pub enum CollectionPanelEvent {
    OpenCollection {
        path: PathBuf,
        name: SharedString,
        variables: HashMap<String, String>,
        scripts: RequestScripts,
    },
    CollectionRenamed {
        previous_path: PathBuf,
        path: PathBuf,
        name: SharedString,
    },
    CollectionDeleted {
        path: PathBuf,
    },
    RequestRelocated {
        id: SharedString,
        previous_path: PathBuf,
        path: PathBuf,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
    },
    OpenRequest {
        id: SharedString,
        path: PathBuf,
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
    /// The branch the last click targeted. Its toggle can scroll another row
    /// under the pointer, so a second click still belongs to it unless the
    /// list is scrolled in between.
    pub(super) clicked: Option<PathBuf>,
    /// A clicked collection waits briefly before it expands or collapses,
    /// so a double click can open it without toggling it.
    pending_toggle: Option<(PathBuf, Task<()>)>,
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
            this.select_row(this.selected_row().unwrap_or(0), cx);
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
            clicked: None,
            pending_toggle: None,
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

    /// A click selects a row. Branches expand or collapse and requests open.
    pub(super) fn click(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(row) = self.visible.binary_search(&index) else {
            return;
        };
        let item = &self.tree.items[index];
        let (kind, path) = (item.kind, item.path.clone());

        window.focus(&self.focus, cx);
        self.select_row(row, cx);

        match kind {
            ItemKind::Collection => {
                self.toggle_later(path.clone(), cx);
                self.clicked = Some(path);
            }
            ItemKind::Folder => {
                self.toggle(index, cx);
                self.clicked = Some(path);
            }
            ItemKind::Request(_) => self.open(index, cx),
        }
    }

    /// The second press of a double click opens a collection and leaves the
    /// tree as it was. Folders have no page, so their second click is ignored.
    pub(super) fn double_click(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.clicked.clone() else {
            return;
        };
        let Some(index) = self.tree.index_of(&path) else {
            return;
        };
        if self.tree.items[index].kind != ItemKind::Collection {
            return;
        }

        match &self.pending_toggle {
            Some((pending, _)) if *pending == path => self.pending_toggle = None,
            // A slow double click finds the collection already toggled.
            _ => self.toggle(index, cx),
        }
        self.open(index, cx);
    }

    fn toggle_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // A newer click stops the previous collection's wait.
        self.toggle_pending(cx);

        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DOUBLE_CLICK_WAIT).await;
            let _ = this.update(cx, |this, cx| this.toggle_pending(cx));
        });
        self.pending_toggle = Some((path, task));
    }

    fn toggle_pending(&mut self, cx: &mut Context<Self>) {
        if let Some((path, _)) = self.pending_toggle.take()
            && let Some(index) = self.tree.index_of(&path)
        {
            self.toggle(index, cx);
        }
    }

    /// Open a collection or request row in a tab; folder rows have no page.
    pub(super) fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.tree.items[index].kind {
            ItemKind::Collection => self.open_collection(index, cx),
            ItemKind::Folder => {}
            ItemKind::Request(_) => self.open_request(index, cx),
        }
    }

    fn open_collection(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = &self.tree.items[index];
        let Some(collection) = self
            .collections
            .collections()
            .iter()
            .find(|collection| collection.path == item.path)
        else {
            return;
        };

        cx.emit(CollectionPanelEvent::OpenCollection {
            path: item.path.clone(),
            name: item.label.clone(),
            variables: collection.local_env().entries.clone(),
            scripts: collection.scripts().clone(),
        });
    }

    fn open_request(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = &self.tree.items[index];
        let Some(file) = self.collections.file(&item.path) else {
            return;
        };
        let (collection, folders) = self.tree.location(index);

        cx.emit(CollectionPanelEvent::OpenRequest {
            id: file.id.clone().into(),
            path: item.path.clone(),
            name: item.label.clone(),
            collection,
            folders,
            request: file.request.clone(),
        });
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

        // Expanding or collapsing from the keyboard overrides a click that is
        // still waiting to.
        if matches!(event.keystroke.key.as_str(), "space" | "left" | "right") {
            self.pending_toggle = None;
            self.clicked = None;
        }

        match event.keystroke.key.as_str() {
            "down" => self.select_row(
                self.selected_row()
                    .map_or(0, |row| (row + 1).min(self.visible.len() - 1)),
                cx,
            ),
            "up" => self.select_row(row.saturating_sub(1), cx),
            "home" => self.select_row(0, cx),
            "end" => self.select_row(self.visible.len() - 1, cx),
            "enter" => self.open(index, cx),
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
            .child(
                div()
                    .debug_selector(|| "collections-search".into())
                    .flex_none()
                    .p_2()
                    // Input actions consume these keys, so transfer focus in capture phase.
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
                    .h_8()
                    .px_4()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child("Collections"),
                    )
                    .child(
                        div()
                            .text_xs()
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
                        .text_xs()
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
                    .key_context("CollectionsSidebar")
                    .on_action(cx.listener(Self::rename_selected))
                    .on_action(cx.listener(Self::delete_selected))
                    .capture_key_down(cx.listener(Self::on_delete_key_down))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        match event.click_count {
                            1 => this.clicked = None,
                            2 if event.button == MouseButton::Left => this.double_click(cx),
                            _ => {}
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
                        this.clicked = None;
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
