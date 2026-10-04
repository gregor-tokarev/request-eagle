use std::{mem, path::Path};

use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::actions::{CloseTab, NewGrpcTab, NewTab, NewWebSocketTab, RenameTab, SaveRequest};
use crate::environment_picker::{CreateEnvironmentRequested, EnvironmentPicker};
use crate::flow_panel::FlowPanel;
use crate::history_panel::{HistoryPanel, short_address};
use crate::save_request;
use crate::session::{SavedFile, SavedTab};
use collection::{
    CollectionEditError, Collections, CollectionsEvent, SavedLocation, directory_name,
};
use request_eagle_theme::{method_label, protocol_icon};
use tab_ui::{
    CollectionPage, CollectionRunner, CookiePage, EnvironmentEditor, Environments,
    EnvironmentsEvent, FlowEditor, GrpcDraft, RequestDraft, RequestSent, RunCollection,
    SaveCollection, Storage, WebSocketDraft,
};

// Rendering and virtualization share the same relative geometry at every zoom.
const TAB_WIDTH: Rems = rems(12.);
const TAB_HEIGHT: Rems = rems(2.);
pub(crate) const TAB_BAR_HEIGHT: Rems = rems(2.5);

/// The content of a tab. Each tab owns its page entity, preserving page state
/// when switching tabs.
#[derive(Clone)]
pub(crate) enum Page {
    Request(Entity<RequestDraft>),
    Grpc(Entity<GrpcDraft>),
    WebSocket(Entity<WebSocketDraft>),
    Flow(Entity<FlowEditor>),
    Collection(Entity<CollectionPage>),
    Runner(Entity<CollectionRunner>),
    Environment(Entity<EnvironmentEditor>),
    Cookies(Entity<CookiePage>),
}

impl Page {
    fn is_dirty(&self, cx: &App) -> bool {
        match self {
            Page::Request(draft) => draft.read(cx).is_dirty(),
            Page::Grpc(draft) => draft.read(cx).is_dirty(),
            Page::WebSocket(draft) => draft.read(cx).is_dirty(),
            Page::Flow(editor) => editor.read(cx).is_dirty(),
            Page::Collection(page) => page.read(cx).is_dirty(),
            Page::Environment(editor) => editor.read(cx).is_dirty(),
            // A run's results are not saved.
            Page::Runner(_) | Page::Cookies(_) => false,
        }
    }

    /// The method or protocol shown before a request tab's title.
    fn label(&self, cx: &App) -> Option<&'static str> {
        match self {
            Page::Request(draft) => Some(draft.read(cx).request.method.as_str()),
            Page::Grpc(_) => Some("gRPC"),
            Page::WebSocket(_) => Some("WS"),
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => None,
        }
    }

    /// Where a request tab's file is saved.
    pub(crate) fn location<'a>(&self, cx: &'a App) -> Option<&'a SavedLocation> {
        match self {
            Page::Request(draft) => draft.read(cx).storage.location(),
            Page::Grpc(draft) => draft.read(cx).storage.location(),
            Page::WebSocket(draft) => draft.read(cx).storage.location(),
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => None,
        }
    }

    pub(crate) fn is_request(&self) -> bool {
        matches!(self, Page::Request(_) | Page::Grpc(_) | Page::WebSocket(_))
    }

    /// Requests and flows are named in their tabs; other tabs after what
    /// they show.
    fn is_renamable(&self) -> bool {
        self.is_request() || matches!(self, Page::Flow(_))
    }

    /// HTTP requests copy as cURL and gRPC requests as grpcurl.
    pub(crate) fn can_copy_as_command(&self) -> bool {
        matches!(self, Page::Request(_) | Page::Grpc(_))
    }

    /// Name a request tab's request before it is saved.
    fn set_name(&self, name: SharedString, cx: &mut App) {
        match self {
            Page::Request(draft) => draft.update(cx, |draft, cx| draft.set_name(name, cx)),
            Page::Grpc(draft) => draft.update(cx, |draft, cx| draft.set_name(name, cx)),
            Page::WebSocket(draft) => draft.update(cx, |draft, cx| draft.set_name(name, cx)),
            // Flows are always saved, so they are renamed in the sidebar.
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => {}
        }
    }

    /// The tab as the next launch reopens it. `dirty` is the tab's marker of
    /// unsaved changes. The session is saved while tabs are edited, so a
    /// request is copied only when the session keeps it.
    fn saved(&self, title: &SharedString, dirty: bool, cx: &App) -> SavedTab {
        fn request<R: Clone + Into<request::Request>>(
            title: &SharedString,
            storage: &Storage,
            request: &R,
            dirty: bool,
        ) -> SavedTab {
            SavedTab::Request {
                title: title.to_string(),
                file: storage.location().map(|location| SavedFile {
                    path: location.path.clone(),
                    id: location.id.clone(),
                    collection: location.collection.clone(),
                }),
                name: match storage {
                    Storage::Unsaved { name } => name.as_ref().map(ToString::to_string),
                    Storage::Saved(_) => None,
                },
                draft: (storage.location().is_none() || dirty).then(|| request.clone().into()),
            }
        }

        match self {
            Page::Request(draft) => {
                let draft = draft.read(cx);
                request(title, &draft.storage, &draft.request, dirty)
            }
            Page::Grpc(draft) => {
                let draft = draft.read(cx);
                request(title, &draft.storage, &draft.request, dirty)
            }
            Page::WebSocket(draft) => {
                let draft = draft.read(cx);
                request(title, &draft.storage, &draft.request, dirty)
            }
            Page::Flow(editor) => {
                let editor = editor.read(cx);
                SavedTab::Flow {
                    path: editor.path.clone(),
                    id: editor.id.to_string(),
                    draft: dirty.then(|| Box::new(editor.flow().clone())),
                }
            }
            Page::Collection(page) => SavedTab::Collection {
                path: page.read(cx).path.clone(),
            },
            Page::Runner(runner) => SavedTab::Runner {
                path: runner.read(cx).path.clone(),
            },
            Page::Environment(editor) => SavedTab::Environment {
                name: editor.read(cx).name.to_string(),
            },
            Page::Cookies(_) => SavedTab::Cookies,
        }
    }

    /// The name a page gives its tab. Pages without one keep the title their
    /// tab opened with, such as Untitled 2.
    fn title(&self, cx: &App) -> Option<SharedString> {
        match self {
            Page::Request(draft) => draft.read(cx).storage.name(),
            Page::Grpc(draft) => draft.read(cx).storage.name(),
            Page::WebSocket(draft) => draft.read(cx).storage.name(),
            Page::Flow(editor) => Some(editor.read(cx).name.clone()),
            Page::Collection(page) => Some(page.read(cx).name().to_owned().into()),
            Page::Runner(runner) => Some(runner.read(cx).name().clone()),
            Page::Environment(editor) => Some(editor.read(cx).name.clone()),
            Page::Cookies(_) => None,
        }
    }

    /// A request tab's request, as edited.
    fn request(&self, cx: &App) -> Option<request::Request> {
        match self {
            Page::Request(draft) => Some(draft.read(cx).request.clone().into()),
            Page::Grpc(draft) => Some(draft.read(cx).request.clone().into()),
            Page::WebSocket(draft) => Some(draft.read(cx).request.clone().into()),
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => None,
        }
    }

    fn icon(&self) -> Option<&'static str> {
        match self {
            Page::Request(_) | Page::Grpc(_) | Page::WebSocket(_) => None,
            Page::Flow(_) => Some("icons/workflow.svg"),
            Page::Collection(_) => Some("icons/package.svg"),
            Page::Runner(_) => Some("icons/square-play.svg"),
            Page::Environment(_) => Some("icons/globe.svg"),
            Page::Cookies(_) => Some("icons/cookie.svg"),
        }
    }

    /// Redraw the tab strip only when the tab's title, label or dirty marker
    /// changes. Every change is reported, so the session can be saved.
    fn observe(&self, id: u64, cx: &mut Context<MainView>) -> Subscription {
        let on_change = move |this: &mut MainView, cx: &mut Context<MainView>| {
            let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            cx.emit(TabEdited);

            let title = tab.page.title(cx).unwrap_or_else(|| tab.title.clone());
            let label = tab.page.label(cx);
            let dirty = tab.page.is_dirty(cx);

            if tab.title != title || tab.label != label || tab.dirty != dirty {
                tab.title = title;
                tab.label = label;
                tab.dirty = dirty;
                cx.notify();
            }
        };

        match self {
            Page::Request(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::Grpc(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::WebSocket(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::Flow(editor) => cx.observe(editor, move |this, _, cx| on_change(this, cx)),
            Page::Collection(page) => cx.observe(page, move |this, _, cx| on_change(this, cx)),
            Page::Runner(runner) => cx.observe(runner, move |this, _, cx| on_change(this, cx)),
            Page::Environment(editor) => cx.observe(editor, move |this, _, cx| on_change(this, cx)),
            Page::Cookies(page) => cx.observe(page, move |this, _, cx| on_change(this, cx)),
        }
    }

    /// Keep the requests a request tab sends in history.
    fn record_history(
        &self,
        history: Entity<HistoryPanel>,
        cx: &mut Context<MainView>,
    ) -> Option<Subscription> {
        fn record<T: EventEmitter<RequestSent>>(
            page: &Entity<T>,
            history: Entity<HistoryPanel>,
            cx: &mut Context<MainView>,
        ) -> Subscription {
            cx.subscribe(page, move |_, _, sent: &RequestSent, cx| {
                history.update(cx, |history, cx| history.record(sent, cx));
            })
        }

        match self {
            Page::Request(draft) => Some(record(draft, history, cx)),
            Page::Grpc(draft) => Some(record(draft, history, cx)),
            Page::WebSocket(draft) => Some(record(draft, history, cx)),
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => None,
        }
    }

    fn prepare(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Request(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::Grpc(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::WebSocket(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::Flow(editor) => editor.update(cx, |editor, cx| editor.prepare(window, cx)),
            Page::Collection(page) => page.update(cx, |page, cx| page.prepare(window, cx)),
            Page::Runner(runner) => runner.update(cx, |runner, cx| runner.prepare(window, cx)),
            Page::Environment(editor) => editor.update(cx, |editor, cx| editor.prepare(window, cx)),
            Page::Cookies(page) => page.update(cx, |page, cx| page.prepare(window, cx)),
        }
    }

    fn render(&self) -> AnyElement {
        match self {
            // Only the request editor's expensive children are cached; selecting
            // a response must not invalidate a cache around the entire page.
            Page::Request(draft) => draft.clone().into_any_element(),
            Page::Grpc(draft) => draft.clone().into_any_element(),
            // The message log changes while streaming; the draft caches its controls.
            Page::WebSocket(draft) => draft.clone().into_any_element(),
            // The canvas redraws while it is dragged or runs.
            Page::Flow(editor) => editor.clone().into_any_element(),
            Page::Collection(page) => page
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Page::Runner(runner) => runner
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Page::Environment(editor) => editor
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Page::Cookies(page) => page
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
        }
    }
}

pub(crate) struct PageTab {
    pub(crate) id: u64,
    pub(crate) title: SharedString,
    pub(crate) label: Option<&'static str>,
    dirty: bool,
    pub(crate) page: Page,
    /// The history entry the tab was opened from.
    history: Option<String>,
    _subscriptions: Vec<Subscription>,
}

/// A request tab whose name is being edited in place.
struct TabRename {
    tab: u64,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// The page of a tab changed, such as a request being edited in it.
pub(crate) struct TabEdited;

impl EventEmitter<TabEdited> for MainView {}

pub(crate) struct MainView {
    pub(crate) tabs: Vec<PageTab>,
    pub(crate) selected: Option<usize>,
    next_id: u64,
    scroll: ScrollHandle,
    scroll_to_tab: Option<usize>,
    focus: FocusHandle,
    pending_close: Option<u64>,
    /// The request tab that could not be saved, because its request was
    /// changed outside the app. It asks whose changes to keep.
    changed_on_disk: Option<u64>,
    rename: Option<TabRename>,
    save_error: Option<String>,
    variable_sessions: environment::EnvironmentSessions,
    pub(crate) environments: Entity<Environments>,
    environment_picker: Entity<EnvironmentPicker>,
    collections: Entity<Collections>,
    /// Stores saved flows.
    flows: Entity<FlowPanel>,
    /// Keeps the requests that tabs send.
    history: Entity<HistoryPanel>,
    _environment_subscriptions: [Subscription; 2],
    _collections_subscription: Subscription,
}

impl MainView {
    /// Reopens `tabs`, the tabs open when the app last closed, or opens an
    /// empty request when none of them can be opened.
    // The sidebar's sections are handed over once, at startup.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        environments: Entity<Environments>,
        collections: Entity<Collections>,
        flows: Entity<FlowPanel>,
        history: Entity<HistoryPanel>,
        tabs: Vec<SavedTab>,
        selected_tab: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let environment_picker =
            cx.new(|cx| EnvironmentPicker::new(environments.clone(), window, cx));
        let picker_subscription = cx.subscribe_in(
            &environment_picker,
            window,
            |this, _, _: &CreateEnvironmentRequested, window, cx| {
                this.create_environment(window, cx)
            },
        );
        let environments_subscription = cx.subscribe(&environments, Self::on_environments_event);
        let collections_subscription =
            cx.subscribe_in(&collections, window, Self::on_collections_event);

        let mut view = Self {
            tabs: Vec::new(),
            selected: None,
            next_id: 1,
            scroll: ScrollHandle::new(),
            scroll_to_tab: None,
            focus: cx.focus_handle(),
            pending_close: None,
            changed_on_disk: None,
            rename: None,
            save_error: None,
            variable_sessions: environment::EnvironmentSessions::default(),
            environments,
            environment_picker,
            collections,
            flows,
            history,
            _environment_subscriptions: [picker_subscription, environments_subscription],
            _collections_subscription: collections_subscription,
        };

        let mut selected = None;
        for (index, tab) in tabs.into_iter().enumerate() {
            if view.restore_tab(tab, window, cx) && selected_tab == Some(index) {
                selected = view.selected;
            }
        }

        if let Some(index) = selected {
            view.select_tab(index, cx);
        }
        if view.tabs.is_empty() {
            view.new_tab(cx);
        }

        view
    }

    /// Reopen a tab saved when the app closed, selecting it, and tell whether
    /// it opened. A saved request or collection that is gone stays closed,
    /// unless the tab kept unsaved changes to it.
    fn restore_tab(&mut self, tab: SavedTab, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match tab {
            SavedTab::Request {
                title,
                file,
                name: unsaved_name,
                draft,
            } => {
                // The file must still hold the same request, in the draft's
                // protocol.
                let saved = file.as_ref().and_then(|file| {
                    let (location, saved) = self.collections.read(cx).request(&file.path)?;
                    let same_protocol = draft.as_ref().is_none_or(|draft| {
                        mem::discriminant(draft) == mem::discriminant(&saved.request)
                    });

                    (location.id == file.id && same_protocol)
                        .then(|| (location, saved.request.clone()))
                });

                match (saved, draft) {
                    (Some((location, request)), draft) => {
                        let title = location.name.clone().into();
                        self.restore_request(title, draft, request, Storage::Saved(location), cx);
                    }
                    // Changes to a request whose file is gone reopen unsaved,
                    // still finding the files they refer to.
                    (None, Some(draft)) => {
                        let draft = match &file {
                            Some(file) => draft.resolved_from(&file.collection),
                            None => draft,
                        };
                        let name = unsaved_name.or_else(|| file.is_some().then(|| title.clone()));
                        self.open_unsaved_copy(title.into(), draft, name.map(Into::into), cx);
                    }
                    (None, None) => return false,
                }
            }
            SavedTab::Collection { path } => return self.open_collection(&path, window, cx),
            SavedTab::Runner { path } => return self.open_runner(&path, cx),
            SavedTab::Environment { name } => {
                let Some(name) = self
                    .environments
                    .read(cx)
                    .names()
                    .iter()
                    .find(|saved| **saved == name)
                    .cloned()
                else {
                    return false;
                };
                let environments = self.environments.clone();
                let editor = cx.new(|cx| EnvironmentEditor::new(name.clone(), environments, cx));

                self.open_tab(name, Page::Environment(editor), cx);
            }
            SavedTab::Flow { path, id, draft } => {
                let Some(saved) = self
                    .flows
                    .read(cx)
                    .get(&path)
                    .filter(|saved| saved.id == id)
                    .cloned()
                else {
                    return false;
                };

                let index = self.open_flow(saved, cx);
                if let (Some(draft), Page::Flow(editor)) = (draft, &self.tabs[index].page) {
                    editor.update(cx, |editor, cx| editor.restore_draft(*draft, cx));
                }
            }
            SavedTab::Cookies => self.open_cookies(cx),
        }

        true
    }

    /// Open a request tab that shows `draft`, or the saved request when there
    /// is no draft. Its changes are measured from `saved`.
    fn restore_request(
        &mut self,
        title: SharedString,
        draft: Option<request::Request>,
        saved: request::Request,
        storage: Storage,
        cx: &mut Context<Self>,
    ) -> usize {
        let index = match draft.unwrap_or_else(|| saved.clone()) {
            request::Request::Http(request) => self.open_draft(title, request, storage, cx),
            request::Request::Grpc(request) => self.open_grpc_draft(title, request, storage, cx),
            request::Request::WebSocket(request) => {
                self.open_websocket(title, request, storage, cx)
            }
        };

        match (&self.tabs[index].page, saved) {
            (Page::Request(draft), request::Request::Http(saved)) => {
                draft.update(cx, |draft, cx| draft.mark_saved(saved, cx));
            }
            (Page::Grpc(draft), request::Request::Grpc(saved)) => {
                draft.update(cx, |draft, cx| draft.mark_saved(saved, cx));
            }
            (Page::WebSocket(draft), request::Request::WebSocket(saved)) => {
                draft.update(cx, |draft, cx| draft.mark_saved(saved, cx));
            }
            _ => {}
        }

        index
    }

    /// Open a request's changes as an unsaved request, such as one whose file
    /// is gone. They are measured from an empty request of its protocol.
    fn open_unsaved_copy(
        &mut self,
        title: SharedString,
        draft: request::Request,
        name: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> usize {
        let empty = match &draft {
            request::Request::Http(_) => request::Request::Http(Default::default()),
            request::Request::Grpc(_) => request::Request::Grpc(Default::default()),
            request::Request::WebSocket(_) => request::Request::WebSocket(Default::default()),
        };

        self.restore_request(title, Some(draft), empty, Storage::Unsaved { name }, cx)
    }

    /// The open tabs, as the next launch reopens them.
    pub(crate) fn saved_tabs(&self, cx: &App) -> Vec<SavedTab> {
        self.tabs
            .iter()
            .map(|tab| tab.page.saved(&tab.title, tab.dirty, cx))
            .collect()
    }

    fn open_tab(
        &mut self,
        title: impl Into<SharedString>,
        page: Page,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_id;
        let mut subscriptions = vec![page.observe(id, cx)];
        subscriptions.extend(page.record_history(self.history.clone(), cx));

        self.tabs.push(PageTab {
            id,
            title: page.title(cx).unwrap_or_else(|| title.into()),
            label: page.label(cx),
            dirty: page.is_dirty(cx),
            page,
            history: None,
            _subscriptions: subscriptions,
        });
        self.next_id += 1;

        let index = self.tabs.len() - 1;
        self.select_tab(index, cx);

        index
    }

    /// Show the saved collection or request at `path`, reusing its tab when
    /// it is open.
    pub(crate) fn open_saved(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let request = self
            .collections
            .read(cx)
            .request(path)
            .map(|(location, file)| (location, file.request.clone()));

        match request {
            Some((location, request)) => self.open_request(location, &request, cx),
            None => {
                self.open_collection(path, window, cx);
            }
        }
    }

    /// Show a saved request, reusing its tab when it is open.
    fn open_request(
        &mut self,
        location: SavedLocation,
        request: &request::Request,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.request_tab(&location.path, &location.id, cx) {
            self.set_request_location(index, location, cx);
            self.select_tab(index, cx);
            return;
        }

        let title: SharedString = location.name.clone().into();
        let storage = Storage::Saved(location);
        match request {
            request::Request::Http(request) => {
                self.open_draft(title, request.clone(), storage, cx);
            }
            request::Request::Grpc(request) => {
                self.open_grpc_draft(title, request.clone(), storage, cx);
            }
            request::Request::WebSocket(request) => {
                self.open_websocket(title, request.clone(), storage, cx);
            }
        }
    }

    /// Follow collections, folders and requests renamed, moved or deleted in
    /// the sidebar or a tab.
    fn on_collections_event(
        &mut self,
        _: &Entity<Collections>,
        event: &CollectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CollectionsEvent::CollectionRenamed {
                previous_path,
                path,
            } => self.relocate_collection(previous_path, path, window, cx),
            CollectionsEvent::FolderRelocated {
                previous_path,
                path,
                collection,
            } => self.relocate_runners(previous_path, path, collection, cx),
            CollectionsEvent::RequestRelocated {
                previous_path,
                location,
            } => self.relocate_request(previous_path, location.clone(), cx),
            CollectionsEvent::Deleted(path) => self.close_deleted(path, cx),
            CollectionsEvent::Created(_) | CollectionsEvent::RequestSaved(_) => {}
        }
    }

    fn relocate_request(
        &mut self,
        previous_path: &Path,
        location: SavedLocation,
        cx: &mut Context<Self>,
    ) {
        for tab in &self.tabs {
            if let Page::Runner(runner) = &tab.page {
                runner.update(cx, |runner, cx| {
                    runner.relocate_request(
                        &location.id,
                        &location.path,
                        location.name.clone().into(),
                        location.folders().into_iter().map(Into::into).collect(),
                        cx,
                    )
                });
            }
        }

        if let Some(index) = self.request_tab(previous_path, &location.id, cx) {
            self.set_request_location(index, location, cx);
            cx.notify();
        }
    }

    fn request_tab(&self, path: &Path, id: &str, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.page
                .location(cx)
                .is_some_and(|location| location.path == path && location.id == id)
        })
    }

    fn set_request_location(
        &mut self,
        index: usize,
        location: SavedLocation,
        cx: &mut Context<Self>,
    ) {
        match &self.tabs[index].page {
            Page::Request(draft) => draft.update(cx, |draft, cx| draft.set_location(location, cx)),
            Page::Grpc(draft) => draft.update(cx, |draft, cx| draft.set_location(location, cx)),
            Page::WebSocket(draft) => {
                draft.update(cx, |draft, cx| draft.set_location(location, cx))
            }
            Page::Flow(_)
            | Page::Collection(_)
            | Page::Runner(_)
            | Page::Environment(_)
            | Page::Cookies(_) => {}
        }
    }

    /// Show a saved flow, reusing its tab when it is open. Returns the tab's index.
    pub(crate) fn open_flow(&mut self, saved: flow::SavedFlow, cx: &mut Context<Self>) -> usize {
        if let Some(index) = self.flow_tab(&saved.path, &saved.id, cx) {
            self.select_tab(index, cx);
            return index;
        }

        let collections = self.collections.clone();
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let title = saved.name.clone();
        let editor =
            cx.new(|cx| FlowEditor::new(saved, collections, sessions, Some(environments), cx));

        self.open_tab(title, Page::Flow(editor), cx)
    }

    fn flow_tab(&self, path: &Path, id: &str, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| match &tab.page {
            Page::Flow(editor) => {
                let editor = editor.read(cx);
                editor.path == path && editor.id.as_ref() == id
            }
            _ => false,
        })
    }

    /// Follow a flow renamed in the sidebar or its tab.
    pub(crate) fn rename_flow(&mut self, path: &Path, name: SharedString, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            if let Page::Flow(editor) = &tab.page
                && editor.read(cx).path == path
            {
                editor.update(cx, |editor, cx| editor.set_name(name.clone(), cx));
            }
        }
    }

    /// Close the tab of a flow deleted in the sidebar.
    pub(crate) fn close_flow(&mut self, path: &Path, cx: &mut Context<Self>) {
        while let Some(index) = self.tabs.iter().position(|tab| match &tab.page {
            Page::Flow(editor) => editor.read(cx).path == path,
            _ => false,
        }) {
            self.remove_tab(index, cx);
        }
    }

    /// Show a collection's page, reusing its tab when it is open. Tells
    /// whether the collection exists.
    fn open_collection(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(index) = self.collection_tab(path, cx) {
            self.select_tab(index, cx);
            return true;
        }

        let Some(collection) = self.collections.read(cx).collection(path) else {
            return false;
        };
        let name: SharedString = directory_name(path).into();
        let variables = collection.local_env().entries.clone();
        let shared = collection::SharedSettings {
            scripts: collection.scripts().clone(),
            auth: collection.auth().clone(),
        };
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let page = cx.new(|cx| {
            CollectionPage::new(
                path.to_path_buf(),
                name.to_string(),
                variables,
                shared,
                sessions,
                Some(environments),
                cx,
            )
        });
        let index = self.open_tab(name, Page::Collection(page.clone()), cx);
        let id = self.tabs[index].id;
        let subscription = cx.subscribe_in(
            &page,
            window,
            move |this, _, _: &SaveCollection, window, cx| {
                if let Some(index) = this.tabs.iter().position(|tab| tab.id == id) {
                    this.save_tab(index, false, window, cx);
                }
            },
        );
        let run_subscription = cx.subscribe_in(
            &page,
            window,
            |this, page, _: &RunCollection, window, cx| {
                let path = page.read(cx).path.clone();
                this.open_runner(&path, cx);
                this.prepare_active_tab(window, cx);
            },
        );
        self.tabs[index]
            ._subscriptions
            .extend([subscription, run_subscription]);

        true
    }

    /// Show the Collection Runner for a collection's or folder's requests,
    /// reusing its tab when it is open. Tells whether the collection or
    /// folder exists.
    pub(crate) fn open_runner(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let collections = self.collections.read(cx);
        let (Some(collection), Some(requests)) =
            (collections.containing(path), collections.requests_in(path))
        else {
            return false;
        };
        let collection = collection.path.clone();
        let requests: Vec<_> = requests
            .into_iter()
            .map(|(location, file)| {
                let folders = location.folders().into_iter().map(Into::into).collect();
                (file.clone(), folders)
            })
            .collect();

        if let Some(index) = self.runner_tab(path, cx) {
            if let Page::Runner(runner) = &self.tabs[index].page {
                runner.update(cx, |runner, cx| runner.refresh(requests, cx));
            }
            self.select_tab(index, cx);
            return true;
        }

        let name: SharedString = directory_name(path).into();
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let runner = cx.new(|cx| {
            CollectionRunner::new(
                path.to_path_buf(),
                name.clone(),
                collection,
                requests,
                sessions,
                environments,
                cx,
            )
        });
        self.open_tab(name, Page::Runner(runner), cx);

        true
    }

    fn runner_tab(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| match &tab.page {
            Page::Runner(runner) => runner.read(cx).path == path,
            _ => false,
        })
    }

    fn collection_tab(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| match &tab.page {
            Page::Collection(page) => page.read(cx).path == path,
            _ => false,
        })
    }

    /// Close the tabs of a deleted collection, folder or request, so a later
    /// one at the same path cannot reuse their stale settings. Requests with
    /// unsaved changes stay open as unsaved requests instead.
    fn close_deleted(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(index) = self.collection_tab(path, cx) {
            self.remove_tab(index, cx);
        }

        // Its runners and those of what it holds end with it.
        while let Some(index) = self.tabs.iter().position(|tab| match &tab.page {
            Page::Runner(runner) => runner.read(cx).path.starts_with(path),
            _ => false,
        }) {
            self.remove_tab(index, cx);
        }

        for index in (0..self.tabs.len()).rev() {
            let page = &self.tabs[index].page;
            let Some(location) = page
                .location(cx)
                .filter(|location| location.path.starts_with(path))
                .cloned()
            else {
                continue;
            };
            let Some(request) = page.request(cx).filter(|_| page.is_dirty(cx)) else {
                self.remove_tab(index, cx);
                continue;
            };

            // Reopen the changes unsaved in the same place, still finding the
            // files they refer to.
            let selected = self.selected;
            let copy = self.open_unsaved_copy(
                self.tabs[index].title.clone(),
                request.resolved_from(&location.collection),
                Some(location.name.into()),
                cx,
            );
            self.tabs[index] = self.tabs.remove(copy);
            self.selected = selected;
            self.scroll_to_tab = selected;
        }
    }

    fn relocate_collection(
        &mut self,
        previous_path: &Path,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.relocate_runners(previous_path, path, path, cx);

        let Some(index) = self.collection_tab(previous_path, cx) else {
            return;
        };
        if let Page::Collection(page) = &self.tabs[index].page {
            page.update(cx, |page, cx| {
                page.relocate(path.to_path_buf(), directory_name(path), window, cx)
            });
        }
    }

    /// Follow a collection or folder renamed or moved in the runners of it
    /// and of what it holds. `collection` is the directory of the collection
    /// it is in now.
    fn relocate_runners(
        &mut self,
        previous_path: &Path,
        path: &Path,
        collection: &Path,
        cx: &mut Context<Self>,
    ) {
        let name: SharedString = directory_name(path).into();
        for tab in &self.tabs {
            if let Page::Runner(runner) = &tab.page {
                runner.update(cx, |runner, cx| {
                    runner.relocate(previous_path, path, name.clone(), collection, cx)
                });
            }
        }
    }

    pub(crate) fn new_tab(&mut self, cx: &mut Context<Self>) {
        self.open_unsaved_request(Default::default(), cx);
    }

    /// Open an HTTP request that is not saved yet, such as an imported cURL
    /// command. Unless it is empty, closing its tab asks to save it.
    pub(crate) fn open_unsaved_request(
        &mut self,
        request: request::HttpRequest,
        cx: &mut Context<Self>,
    ) {
        let title = format!("Untitled {}", self.next_id);
        self.open_draft(title.into(), request, Storage::Unsaved { name: None }, cx);

        if let Some(Page::Request(draft)) = self.tabs.last().map(|tab| &tab.page) {
            draft.update(cx, |draft, cx| draft.mark_saved(Default::default(), cx));
        }
    }

    pub(crate) fn new_websocket_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_websocket(
            title.into(),
            Default::default(),
            Storage::Unsaved { name: None },
            cx,
        );
    }

    fn open_websocket(
        &mut self,
        title: SharedString,
        request: request::WebSocketRequest,
        storage: Storage,
        cx: &mut Context<Self>,
    ) -> usize {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft =
            cx.new(|cx| WebSocketDraft::new(request, storage, sessions, Some(environments), cx));

        self.open_tab(title, Page::WebSocket(draft), cx)
    }

    fn open_draft(
        &mut self,
        title: SharedString,
        request: request::HttpRequest,
        storage: Storage,
        cx: &mut Context<Self>,
    ) -> usize {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft =
            cx.new(|cx| RequestDraft::new(request, storage, sessions, Some(environments), cx));

        self.open_tab(title, Page::Request(draft), cx)
    }

    pub(crate) fn new_grpc_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_grpc_draft(
            title.into(),
            Default::default(),
            Storage::Unsaved { name: None },
            cx,
        );
    }

    fn open_grpc_draft(
        &mut self,
        title: SharedString,
        request: request::GrpcRequest,
        storage: Storage,
        cx: &mut Context<Self>,
    ) -> usize {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft = cx.new(|cx| GrpcDraft::new(request, storage, sessions, Some(environments), cx));

        self.open_tab(title, Page::Grpc(draft), cx)
    }

    /// Show a request from history as a new unsaved tab, with the response
    /// it received. Its tab is reused while it is open.
    pub(crate) fn open_history(
        &mut self,
        entry: &request_history::Entry,
        record: request_history::Record,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.history.as_ref() == Some(&entry.id))
        {
            self.select_tab(index, cx);
            return;
        }

        let title: SharedString = match short_address(&entry.address) {
            "" => format!("Untitled {}", self.next_id).into(),
            address => address.to_owned().into(),
        };
        let storage = Storage::Unsaved { name: None };
        let index = match record.request {
            request::Request::Http(request) => self.open_draft(title, request, storage, cx),
            request::Request::Grpc(request) => self.open_grpc_draft(title, request, storage, cx),
            request::Request::WebSocket(request) => {
                self.open_websocket(title, request, storage, cx)
            }
        };

        if let Page::Request(draft) = &self.tabs[index].page {
            draft.update(cx, |draft, cx| {
                draft.show_recorded(record.response, record.error, window, cx)
            });
        }

        self.tabs[index].history = Some(entry.id.clone());
    }

    fn environment_tab(&self, name: &str, cx: &App) -> Option<(usize, Entity<EnvironmentEditor>)> {
        self.tabs
            .iter()
            .enumerate()
            .find_map(|(index, tab)| match &tab.page {
                Page::Environment(editor) if editor.read(cx).name == name => {
                    Some((index, editor.clone()))
                }
                _ => None,
            })
    }

    /// Show a global environment's editor, reusing its tab when it is open.
    pub(crate) fn open_environment(
        &mut self,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EnvironmentEditor> {
        let editor = if let Some((index, editor)) = self.environment_tab(&name, cx) {
            self.select_tab(index, cx);
            editor
        } else {
            let environments = self.environments.clone();
            let editor = cx.new(|cx| EnvironmentEditor::new(name.clone(), environments, cx));
            self.open_tab(name, Page::Environment(editor.clone()), cx);
            editor
        };

        self.focus(window, cx);
        editor
    }

    /// Show the cookie jar, reusing its tab when it is open.
    pub(crate) fn open_cookies(&mut self, cx: &mut Context<Self>) {
        let open = self
            .tabs
            .iter()
            .position(|tab| matches!(tab.page, Page::Cookies(_)));

        match open {
            Some(index) => self.select_tab(index, cx),
            None => {
                let page = cx.new(CookiePage::new);
                self.open_tab("Cookies", Page::Cookies(page), cx);
            }
        }
    }

    /// Open the environment's editor with its name selected for renaming.
    pub(crate) fn rename_environment(
        &mut self,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.open_environment(name, window, cx);
        editor.update(cx, |editor, cx| editor.focus_name(window, cx));
    }

    pub(crate) fn create_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self
            .environments
            .update(cx, |environments, cx| environments.create(cx))
        {
            self.rename_environment(name, window, cx);
        }
    }

    fn on_environments_event(
        &mut self,
        _: Entity<Environments>,
        event: &EnvironmentsEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Only the environment's editor renames it, and its tab follows
            // the editor's name.
            EnvironmentsEvent::Renamed { .. } => {}
            EnvironmentsEvent::Deleted(name) => {
                if let Some((index, _)) = self.environment_tab(name, cx) {
                    self.remove_tab(index, cx);
                }
            }
        }

        cx.notify();
    }

    /// Select by zero-based position. Missing positions leave selection unchanged.
    pub(crate) fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        if self.selected != Some(index) {
            self.pending_close = None;
            self.changed_on_disk = None;
            self.save_error = None;
        }

        self.selected = Some(index);
        self.scroll_to_tab = Some(index);

        cx.notify();
    }

    pub(crate) fn cycle_tab(&mut self, previous: bool, cx: &mut Context<Self>) {
        let Some(index) = self.selected else {
            return;
        };

        let count = self.tabs.len();
        let next = if previous {
            (index + count - 1) % count
        } else {
            (index + 1) % count
        };

        self.select_tab(next, cx);
    }

    pub(crate) fn select_last_tab(&mut self, cx: &mut Context<Self>) {
        self.select_tab(self.tabs.len().saturating_sub(1), cx);
    }

    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            if self.pending_close == Some(self.tabs[index].id) {
                self.remove_tab(index, cx);
            } else {
                self.close_tab(index, cx);
            }
        }
    }

    pub(crate) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        if self.tabs[index].page.is_dirty(cx) {
            self.select_tab(index, cx);
            self.pending_close = Some(self.tabs[index].id);
            cx.notify();
            return;
        }

        self.remove_tab(index, cx);
    }

    fn remove_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.pending_close = None;
        self.changed_on_disk = None;
        self.save_error = None;
        self.tabs.remove(index);
        self.selected = self.selected.and_then(|selected| {
            if self.tabs.is_empty() {
                None
            } else if index < selected {
                Some(selected - 1)
            } else {
                Some(selected.min(self.tabs.len() - 1))
            }
        });

        self.scroll_to_tab = self.selected;

        cx.notify();
    }

    pub(crate) fn save_active_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.save_tab(index, false, window, cx);
        }
    }

    /// `overwrite` saves a request over the changes made to it outside the
    /// app, once the user chose to.
    fn save_tab(
        &mut self,
        index: usize,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Saving an unsaved request suggests the name being typed in its tab.
        // A rejected rename keeps its error.
        self.save_error = None;
        self.changed_on_disk = None;
        self.commit_rename(cx);

        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let id = tab.id;

        match tab.page.clone() {
            Page::Environment(editor) => {
                // The editor shows its own save errors next to the variables.
                let saved = editor.update(cx, |editor, cx| editor.save(cx)).is_ok();

                if saved && self.pending_close == Some(id) {
                    self.remove_tab(index, cx);
                }
            }
            Page::Collection(page) => {
                let path = page.read(cx).path.clone();
                let result = page.read(cx).settings().and_then(|settings| {
                    self.collections
                        .update(cx, |collections, cx| {
                            collections.save_collection(
                                &path,
                                &settings.name,
                                settings.variables.iter().cloned().collect(),
                                collection::SharedSettings {
                                    scripts: settings.scripts.clone(),
                                    auth: settings.auth.clone(),
                                },
                                cx,
                            )
                        })
                        .map(|path| (path, settings))
                        .map_err(|error| error.to_string())
                });

                match result {
                    Ok((path, settings)) => {
                        page.update(cx, |page, cx| page.mark_saved(path, settings, cx));
                        self.close_saved_tab(index, window, cx);
                    }
                    Err(error) => {
                        self.save_error = Some(format!("Could not save collection: {error}"))
                    }
                }
            }
            // The jar saves itself whenever it changes.
            Page::Runner(_) | Page::Cookies(_) => {}
            Page::Flow(editor) => {
                let (path, id, flow, check) = {
                    let editor = editor.read(cx);
                    (
                        editor.path.clone(),
                        editor.id.clone(),
                        editor.flow().clone(),
                        editor.check(),
                    )
                };
                let result = check.and_then(|()| {
                    self.flows
                        .update(cx, |flows, _| flows.save(&path, &id, flow.clone()))
                        .map_err(|error| error.to_string())
                });

                match result {
                    Ok(()) => {
                        editor.update(cx, |editor, cx| editor.mark_saved(flow, cx));
                        self.close_saved_tab(index, window, cx);
                    }
                    Err(error) => self.save_error = Some(format!("Could not save flow: {error}")),
                }
            }
            Page::Request(draft) => {
                let request = draft.read(cx).request.clone();
                let storage = draft.read(cx).storage.clone();

                if self.save_request_at(id, storage, request.clone().into(), overwrite, window, cx)
                {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
            Page::Grpc(draft) => {
                let request = draft.read(cx).request.clone();
                let storage = draft.read(cx).storage.clone();

                if self.save_request_at(id, storage, request.clone().into(), overwrite, window, cx)
                {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
            Page::WebSocket(draft) => {
                let request = draft.read(cx).request.clone();
                let storage = draft.read(cx).storage.clone();

                if self.save_request_at(id, storage, request.clone().into(), overwrite, window, cx)
                {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
        }

        cx.notify();
    }

    /// Save a request tab to its file. An unsaved request opens the dialog
    /// that chooses where, suggesting the name given in its tab; that dialog
    /// marks the tab saved itself. A request changed outside the app is
    /// saved only with `overwrite`; without it, the tab asks whose changes
    /// to keep.
    fn save_request_at(
        &mut self,
        tab_id: u64,
        storage: Storage,
        request: request::Request,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let location = match storage {
            Storage::Saved(location) => location,
            Storage::Unsaved { name } => {
                save_request::open(
                    cx.entity(),
                    self.collections.clone(),
                    tab_id,
                    name,
                    request,
                    window,
                    cx,
                );
                return false;
            }
        };

        let result = self.collections.update(cx, |collections, cx| {
            if overwrite {
                collections.overwrite_request(&location.path, &location.id, request, cx)
            } else {
                collections.update_request(&location.path, &location.id, request, cx)
            }
        });

        match &result {
            Ok(()) => {}
            Err(CollectionEditError::ChangedOnDisk) => self.changed_on_disk = Some(tab_id),
            Err(error) => self.save_error = Some(format!("Could not save request: {error}")),
        }

        result.is_ok()
    }

    /// Discard a request tab's changes for the ones made to its file outside
    /// the app.
    fn reload_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.tabs[index];
        let Some(location) = tab.page.location(cx).cloned() else {
            return;
        };
        let closing = self.pending_close == Some(tab.id);

        self.changed_on_disk = None;
        let result = self.collections.update(cx, |collections, cx| {
            collections.reload_request(&location.path, &location.id, cx)
        });
        let request = self
            .collections
            .read(cx)
            .request(&location.path)
            .map(|(location, file)| (location, file.request.clone()));

        match (result, request) {
            // A tab that was being closed has nothing left to save.
            (Ok(()), Some(_)) if closing => self.remove_tab(index, cx),
            // The file's request opens in place of the changed one.
            (Ok(()), Some((location, request))) => {
                let title = location.name.clone().into();
                self.restore_request(title, None, request, Storage::Saved(location), cx);
                self.tabs.swap_remove(index);
                self.select_tab(index, cx);
            }
            (Err(error), _) => {
                self.save_error = Some(format!("Could not reload request: {error}"));
            }
            (Ok(()), None) => {}
        }

        self.focus(window, cx);
        cx.notify();
    }

    /// Attach a new request draft to the file it was saved as.
    pub(crate) fn attach_saved_request(
        &mut self,
        tab_id: u64,
        location: SavedLocation,
        request: &request::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        self.set_request_location(index, location, cx);

        match (&self.tabs[index].page, request) {
            (Page::Request(draft), request::Request::Http(request)) => {
                draft.update(cx, |draft, cx| {
                    // Saving can store body files relative to the collection.
                    draft.set_saved_files(request.body.clone(), window, cx);
                    draft.mark_saved(request.clone(), cx);
                });
            }
            (Page::Grpc(draft), request::Request::Grpc(request)) => {
                draft.update(cx, |draft, cx| {
                    // Saving can store `.proto` paths relative to the collection.
                    draft.set_definition(request.definition.clone(), window, cx);
                    draft.mark_saved(request.clone(), cx);
                });
            }
            (Page::WebSocket(draft), request::Request::WebSocket(request)) => {
                draft.update(cx, |draft, cx| draft.mark_saved(request.clone(), cx));
            }
            _ => {}
        }

        self.save_error = None;
        self.close_saved_tab(index, window, cx);
        cx.notify();
    }

    /// Finish closing a tab whose changes were saved from the close confirmation.
    fn close_saved_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_close == Some(self.tabs[index].id) && !self.tabs[index].page.is_dirty(cx) {
            self.remove_tab(index, cx);
            self.focus(window, cx);
        }
    }

    /// Edit a request tab's name in place. Other tabs are named after what
    /// they show.
    fn begin_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.tabs[index];

        if !tab.page.is_renamable() {
            return;
        }

        let id = tab.id;
        let title = tab.title.clone();

        // Renaming the same tab again keeps the name being typed.
        if let Some(rename) = self.rename.as_ref().filter(|rename| rename.tab == id) {
            rename.input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        self.commit_rename(cx);

        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).default_value(title);
            input.select_all(window, cx);
            input.focus(window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.commit_rename(cx);
                    this.focus(window, cx);
                }
                // Clicking elsewhere keeps the new name, as Enter does.
                InputEvent::Blur => this.commit_rename(cx),
                _ => {}
            },
        );

        self.rename = Some(TabRename {
            tab: id,
            input,
            _subscription: subscription,
        });
        self.scroll_to_tab = Some(index);
        cx.notify();
    }

    fn on_rename_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            self.rename = None;
            self.focus(window, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// Rename a saved request in its collection, as the sidebar does. An
    /// unsaved request keeps the name until it is saved.
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        cx.notify();

        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == rename.tab) else {
            return;
        };
        let name = rename.input.read(cx).value().trim().to_owned();

        if name.is_empty() || name == tab.title.as_ref() {
            return;
        }

        if let Page::Flow(editor) = &tab.page {
            // The tab follows the flow panel's rename event.
            let (path, id) = {
                let editor = editor.read(cx);
                (editor.path.clone(), editor.id.clone())
            };
            let result = self
                .flows
                .update(cx, |flows, cx| flows.rename(&path, &id, &name, cx));

            if let Err(error) = result {
                self.save_error = Some(format!("Could not rename flow: {error}"));
            }
            return;
        }

        match tab.page.location(cx).cloned() {
            // The tab follows the relocation event.
            Some(location) => {
                let result = self.collections.update(cx, |collections, cx| {
                    collections.rename_request(&location.path, &location.id, &name, cx)
                });

                if let Err(error) = result {
                    self.save_error = Some(format!("Could not rename request: {error}"));
                }
            }
            None => tab.page.set_name(name.into(), cx),
        }
    }

    /// Asks whose changes to keep, after a save found the request changed
    /// outside the app.
    fn changed_on_disk_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let changed_tab = |this: &Self| {
            this.tabs
                .iter()
                .position(|tab| Some(tab.id) == this.changed_on_disk)
        };

        h_flex()
            .debug_selector(|| "request-changed-on-disk-prompt".into())
            .flex_none()
            .px_3()
            .py_2()
            .gap_2()
            .bg(cx.theme().muted)
            .child(
                div()
                    .flex_1()
                    .child("This request was changed outside Request Eagle. Keep your changes?"),
            )
            .child(
                Button::new("overwrite-changed-request")
                    .debug_selector(|| "overwrite-changed-request".into())
                    .small()
                    .label("Overwrite")
                    .tooltip("Save your changes over the ones in the file")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(index) = changed_tab(this) {
                            this.save_tab(index, true, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("reload-changed-request")
                    .debug_selector(|| "reload-changed-request".into())
                    .small()
                    .label("Reload")
                    .tooltip("Discard your changes and open the request from its file")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(index) = changed_tab(this) {
                            this.reload_tab(index, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("cancel-changed-request")
                    .debug_selector(|| "cancel-changed-request".into())
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.changed_on_disk = None;
                        this.focus(window, cx);
                        cx.notify();
                    })),
            )
    }

    fn close_confirmation(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .debug_selector(|| "unsaved-request-prompt".into())
            .flex_none()
            .px_3()
            .py_2()
            .gap_2()
            .bg(cx.theme().muted)
            .child(
                div()
                    .flex_1()
                    .child("Save changes before closing this tab?"),
            )
            .child(
                Button::new("save-and-close-request")
                    .debug_selector(|| "save-and-close-request".into())
                    .small()
                    .primary()
                    .label("Save")
                    .tooltip_with_action("Save changes and close", &SaveRequest, Some("Workspace"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.save_active_request(window, cx)),
                    ),
            )
            .child(
                Button::new("discard-request-changes")
                    .debug_selector(|| "discard-request-changes".into())
                    .small()
                    .label("Discard")
                    .tooltip_with_action("Discard changes and close", &CloseTab, Some("Workspace"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(index) = this
                            .tabs
                            .iter()
                            .position(|tab| Some(tab.id) == this.pending_close)
                        {
                            this.remove_tab(index, cx);
                            this.focus(window, cx);
                        }
                    })),
            )
            .child(
                Button::new("cancel-close-request")
                    .debug_selector(|| "cancel-close-request".into())
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pending_close = None;
                        this.save_error = None;
                        this.focus(window, cx);
                        cx.notify();
                    })),
            )
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_active_tab(window, cx);
        window.focus(&self.focus, cx);
    }

    pub(crate) fn open_environment_picker(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.environment_picker
            .update(cx, |picker, cx| picker.open(window, cx));
    }

    pub(crate) fn prepare_active_tab(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.tabs[index].page.prepare(window, cx);
        }
    }

    /// Send the active request. A WebSocket connects, or sends its message
    /// once connected.
    pub(crate) fn send_request(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected.map(|index| &self.tabs[index].page) {
            Some(Page::Request(draft)) => draft.update(cx, |draft, cx| draft.send(window, cx)),
            Some(Page::Grpc(draft)) => draft.update(cx, |draft, cx| draft.send(window, cx)),
            Some(Page::WebSocket(draft)) => draft.update(cx, |draft, cx| draft.send(window, cx)),
            Some(Page::Flow(editor)) => editor.update(cx, |editor, cx| editor.run(window, cx)),
            _ => {}
        }
    }

    pub(crate) fn active_page(&self) -> Option<&Page> {
        self.selected.map(|index| &self.tabs[index].page)
    }

    pub(crate) fn copy_as_command(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_page() {
            Some(Page::Request(draft)) => {
                draft.update(cx, |draft, cx| draft.copy_as_curl(window, cx))
            }
            Some(Page::Grpc(draft)) => {
                draft.update(cx, |draft, cx| draft.copy_as_grpcurl(window, cx))
            }
            _ => {}
        }
    }

    pub(crate) fn focus_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_page() {
            Some(Page::Request(draft)) => draft.update(cx, |draft, cx| draft.focus_url(window, cx)),
            Some(Page::Grpc(draft)) => draft.update(cx, |draft, cx| draft.focus_url(window, cx)),
            Some(Page::WebSocket(draft)) => {
                draft.update(cx, |draft, cx| draft.focus_url(window, cx))
            }
            _ => {}
        }
    }

    /// Choose the protocol of a new tab. The plus button opens HTTP requests.
    fn new_tab_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let focus = self.focus.clone();

        Button::new("new-tab-menu")
            .debug_selector(|| "new-tab-menu".into())
            .ghost()
            .small()
            .flex_none()
            .icon(Icon::new(IconName::ChevronDown).size_3())
            .accessibility_label("New tab of a type")
            .dropdown_menu(move |menu, _, cx| {
                let http_view = view.clone();
                let grpc_view = view.clone();
                let websocket_view = view.clone();

                menu.action_context(focus.clone())
                    .item(
                        PopupMenuItem::new("HTTP Request")
                            .icon(protocol_icon("HTTP", cx))
                            .action(Box::new(NewTab))
                            .on_click(move |_, window, cx| {
                                let _ = http_view.update(cx, |this, cx| {
                                    this.new_tab(cx);
                                    this.focus(window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("gRPC Request")
                            .icon(protocol_icon("gRPC", cx))
                            .action(Box::new(NewGrpcTab))
                            .on_click(move |_, window, cx| {
                                let _ = grpc_view.update(cx, |this, cx| {
                                    this.new_grpc_tab(cx);
                                    this.focus(window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("WebSocket Request")
                            .icon(protocol_icon("WS", cx))
                            .action(Box::new(NewWebSocketTab))
                            .on_click(move |_, window, cx| {
                                let _ = websocket_view.update(cx, |this, cx| {
                                    this.new_websocket_tab(cx);
                                    this.focus(window, cx);
                                });
                            }),
                    )
            })
    }

    fn tab_strip(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let gap = window.rem_size() * 0.25;
        let tab_width = TAB_WIDTH.to_pixels(window.rem_size());
        let tab_height = TAB_HEIGHT.to_pixels(window.rem_size());
        let stride = tab_width + gap;
        let content_width = tab_width * self.tabs.len() + gap * self.tabs.len().saturating_sub(1);

        Tabs::new("page-tabs")
            .min_w_0()
            .flex_shrink(1.)
            .flex()
            .overflow_x_scroll()
            .track_scroll(&self.scroll)
            .child(
                // Reserve the full scroll extent, but build and lay out only the
                // visible tabs. GPUI's uniform_list only supports vertical lists.
                canvas(
                    cx.processor(move |this, bounds: Bounds<Pixels>, window, cx| {
                        // The parent has its current viewport and clamped scroll
                        // offset now, including on the first frame and after resize.
                        let viewport = this.scroll.bounds();
                        let mut left = -this.scroll.offset().x;

                        if let Some(index) = this.scroll_to_tab.take() {
                            let tab_left = stride * index;
                            let tab_right = tab_left + tab_width;

                            if tab_left < left || tab_width > viewport.size.width {
                                left = tab_left;
                            } else if tab_right > left + viewport.size.width {
                                left = tab_right - viewport.size.width;
                            }
                        }

                        left =
                            left.clamp(px(0.), (content_width - viewport.size.width).max(px(0.)));
                        this.scroll.set_offset(point(-left, px(0.)));

                        let first = (left / stride).floor() as usize;
                        let end = (((left + viewport.size.width) / stride).ceil() as usize)
                            .min(this.tabs.len());
                        let mut tabs = Vec::with_capacity(end.saturating_sub(first));

                        for index in first..end {
                            let mut tab = this.tab(index, &this.tabs[index], cx).into_any_element();
                            tab.layout_as_root(
                                size(
                                    AvailableSpace::Definite(tab_width),
                                    AvailableSpace::Definite(tab_height),
                                ),
                                window,
                                cx,
                            );
                            // Use the new offset immediately, so keyboard jumps
                            // reveal the selected tab in this frame.
                            tab.prepaint_at(
                                point(viewport.left() - left + stride * index, bounds.top()),
                                window,
                                cx,
                            );
                            tabs.push(tab);
                        }

                        tabs
                    }),
                    |_, tabs, window, cx| {
                        for mut tab in tabs {
                            tab.paint(window, cx);
                        }
                    },
                )
                .flex_none()
                .w(content_width)
                .h(TAB_HEIGHT),
            )
    }

    fn tab(&self, index: usize, tab: &PageTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.selected == Some(index);
        let id = tab.id;

        Tab::new(("page-tab", id))
            .debug_selector(move || format!("page-tab-{id}"))
            .group("page-tab")
            .selected(selected)
            .accessibility_label(if tab.dirty {
                format!("{}, unsaved changes", tab.title).into()
            } else {
                tab.title.clone()
            })
            .set_position(index + 1, self.tabs.len())
            .flex_none()
            .w(TAB_WIDTH)
            .h(TAB_HEIGHT)
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius_tokens().md)
            .text_sm()
            .text_color(cx.theme().tab_foreground)
            .when(selected, |this| {
                this.bg(cx.theme().tokens.tab_active.background)
                    .text_color(cx.theme().tab_active_foreground)
            })
            .hover(|this| {
                if selected {
                    this
                } else {
                    this.bg(cx.theme().muted)
                }
            })
            .when_some(tab.page.icon(), |this, icon| {
                this.child(
                    Icon::default()
                        .path(icon)
                        .size(rems(0.875))
                        .flex_none()
                        .text_color(cx.theme().muted_foreground),
                )
            })
            .when_some(tab.label, |this, label| {
                this.child(
                    div()
                        .debug_selector(move || format!("tab-method-{id}"))
                        .flex_none()
                        .child(method_label(label, cx)),
                )
            })
            .child(
                if let Some(rename) = self.rename.as_ref().filter(|rename| rename.tab == id) {
                    div()
                        .id("tab-rename-editor")
                        .debug_selector(|| "tab-rename-editor".into())
                        .flex_1()
                        .min_w_0()
                        .capture_key_down(cx.listener(Self::on_rename_key_down))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(Input::new(&rename.input).small().aria_label("Request name"))
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(tab.title.clone())
                        .into_any_element()
                },
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size_5()
                    .when(tab.dirty, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover("page-tab", |this| this.invisible())
                                .child(
                                    div()
                                        .debug_selector(move || format!("tab-dirty-{id}"))
                                        .size_2()
                                        .rounded(cx.theme().radius_full())
                                        .bg(cx.theme().warning),
                                ),
                        )
                    })
                    .child(
                        div()
                            .size_full()
                            .invisible()
                            .group_hover("page-tab", |this| this.visible())
                            .child(
                                Button::new(("close-tab", id))
                                    .debug_selector(move || format!("close-tab-{id}"))
                                    .ghost()
                                    .xsmall()
                                    .size_5()
                                    .icon(Icon::new(IconName::Close).size_3())
                                    .accessibility_label(format!("Close {}", tab.title))
                                    .tooltip_with_action("Close tab", &CloseTab, Some("Workspace"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(index, cx);
                                        this.focus(window, cx);
                                    })),
                            ),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.select_tab(index, cx);
                this.focus(window, cx);

                if event.click_count() == 2 {
                    this.begin_rename(index, window, cx);
                }
            }))
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only request tabs can be renamed, so other tabs leave Rename tab
        // out of the palette.
        let renamable = self
            .selected
            .filter(|&index| self.tabs[index].page.is_renamable());

        v_flex()
            .debug_selector(|| "main-view".into())
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .when_some(renamable, |this, index| {
                this.on_action(cx.listener(move |this, _: &RenameTab, window, cx| {
                    this.begin_rename(index, window, cx);
                }))
            })
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .debug_selector(|| "main-tab-bar".into())
                    .flex_none()
                    .h(TAB_BAR_HEIGHT)
                    // The environment picker runs to the edge.
                    .pl_1()
                    .gap_1()
                    .bg(cx.theme().tab_bar)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(self.tab_strip(window, cx))
                    .child(
                        Button::new("new-tab")
                            .debug_selector(|| "new-tab".into())
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(IconName::Plus)
                            .accessibility_label("New tab")
                            .tooltip_with_action("New tab", &NewTab, Some("Workspace"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_tab(cx);
                                this.focus(window, cx);
                            })),
                    )
                    .child(self.new_tab_menu(cx))
                    .child(div().flex_1())
                    .child(self.environment_picker.clone()),
            )
            // Saving from the close confirmation can find the request changed.
            .when(self.changed_on_disk.is_some(), |this| {
                this.child(self.changed_on_disk_prompt(cx))
            })
            .when(
                self.pending_close.is_some() && self.changed_on_disk.is_none(),
                |this| this.child(self.close_confirmation(cx)),
            )
            .when_some(self.save_error.clone(), |this, error| {
                this.child(
                    div()
                        .debug_selector(|| "request-save-error".into())
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                div()
                    .id("tab-content")
                    .role(Role::TabPanel)
                    .debug_selector(|| "tab-content".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // Focus the page before its controls handle the click, so
                    // clicking an input can keep focus instead of losing it here.
                    .capture_any_mouse_down(cx.listener(
                        |this, event: &MouseDownEvent, window, cx| {
                            if event.button == MouseButton::Left {
                                this.focus(window, cx);
                            }
                        },
                    ))
                    .when_some(self.selected, |this, index| {
                        this.aria_label(self.tabs[index].title.clone())
                            .child(self.tabs[index].page.render())
                    }),
            )
    }
}
