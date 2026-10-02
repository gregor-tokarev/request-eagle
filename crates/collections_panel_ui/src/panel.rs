use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use collection::{CollectionRegistry, FileEntry, MovePlacement, SharedSettings};

use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{HttpRequest, Request};

use super::{
    actions::{DeleteItem, RenameItem},
    editing::RenameEditor,
    tree::{CollectionTree, ItemKind},
};

// Events are passed on one at a time, so boxing the request would not help.
#[allow(clippy::large_enum_variant)]
pub enum CollectionPanelEvent {
    OpenCollection {
        path: PathBuf,
        name: SharedString,
        variables: HashMap<String, String>,
        /// The scripts and authorization it shares with its requests.
        shared: SharedSettings,
    },
    CollectionRenamed {
        previous_path: PathBuf,
        path: PathBuf,
        name: SharedString,
    },
    CollectionDeleted {
        path: PathBuf,
    },
    /// A folder was renamed or moved. Its requests' relocations follow.
    FolderRelocated {
        previous_path: PathBuf,
        path: PathBuf,
        name: SharedString,
        /// The directory of the collection it is in now.
        collection: PathBuf,
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
    OpenFlow {
        id: SharedString,
        path: PathBuf,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
        flow: flow::Flow,
    },
    /// A request imported without saving it, such as a pasted cURL command.
    OpenUnsavedRequest(HttpRequest),
    /// Imported environments were added to the environments directory.
    EnvironmentsImported,
    /// Open the Collection Runner for a collection's or folder's requests.
    RunRequests {
        /// The collection or folder.
        path: PathBuf,
        name: SharedString,
        /// The directory of the collection that stores the requests.
        collection: PathBuf,
        /// In tree order.
        requests: Vec<RunnableRequest>,
    },
}

/// A saved request of a collection or folder that is run.
#[derive(Clone)]
pub struct RunnableRequest {
    pub file: FileEntry,
    /// The folders between the collection and the request.
    pub folders: Vec<SharedString>,
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
            clicked_branch: None,
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

    pub fn collection_count(&self) -> usize {
        self.tree.roots.len()
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

    /// The saved collections, for pages that read them, such as flows
    /// choosing the requests they send.
    pub fn registry(&self) -> &CollectionRegistry {
        &self.collections
    }

    /// Open a collection, request or flow row in a tab; folder rows have no page.
    pub(super) fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(event) = self.open_event(index) {
            cx.emit(event);
        }
    }

    /// The event that opens a collection or request row's page.
    pub(super) fn open_event(&self, index: usize) -> Option<CollectionPanelEvent> {
        let item = &self.tree.items[index];

        match item.kind {
            ItemKind::Collection => {
                let collection = self
                    .collections
                    .collections()
                    .iter()
                    .find(|collection| collection.path == item.path)?;

                Some(CollectionPanelEvent::OpenCollection {
                    path: item.path.clone(),
                    name: item.label.clone(),
                    variables: collection.local_env().entries.clone(),
                    shared: SharedSettings {
                        scripts: collection.scripts().clone(),
                        auth: collection.auth().clone(),
                    },
                })
            }
            ItemKind::Folder => None,
            ItemKind::Flow => {
                let entry = self.collections.flow(&item.path)?;
                let (collection, folders) = self.tree.location(index);

                Some(CollectionPanelEvent::OpenFlow {
                    id: entry.id.clone().into(),
                    path: item.path.clone(),
                    name: item.label.clone(),
                    collection,
                    folders,
                    flow: entry.flow.clone(),
                })
            }
            ItemKind::Request(_) => {
                let file = self.collections.file(&item.path)?;
                let (collection, folders) = self.tree.location(index);

                Some(CollectionPanelEvent::OpenRequest {
                    id: file.id.clone().into(),
                    path: item.path.clone(),
                    name: item.label.clone(),
                    collection,
                    folders,
                    request: file.request.clone(),
                })
            }
        }
    }

    /// The event that runs a collection or folder row's requests.
    pub(super) fn run_event(&self, index: usize) -> Option<CollectionPanelEvent> {
        let item = &self.tree.items[index];
        if !item.is_branch() {
            return None;
        }

        let mut root = index;
        while let Some(parent) = self.tree.items[root].parent {
            root = parent;
        }

        let requests = (index + 1..item.end)
            .filter(|&child| matches!(self.tree.items[child].kind, ItemKind::Request(_)))
            .filter_map(|child| {
                let file = self.collections.file(&self.tree.items[child].path)?;

                Some(RunnableRequest {
                    file: file.clone(),
                    folders: self.tree.location(child).1,
                })
            })
            .collect();

        Some(CollectionPanelEvent::RunRequests {
            path: item.path.clone(),
            name: item.label.clone(),
            collection: self.tree.items[root].path.clone(),
            requests,
        })
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
            .when(!self.collections.skipped().is_empty(), |this| {
                this.child(
                    h_flex()
                        .debug_selector(|| "collections-skipped".into())
                        .px_3()
                        .pb_2()
                        .gap_1()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::TriangleAlert).xsmall())
                        .child(match self.collections.skipped().len() {
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
