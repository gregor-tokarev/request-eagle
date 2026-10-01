use std::{collections::HashMap, path::Path};

use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::actions::{CloseTab, NewGrpcTab, NewTab, NewWebSocketTab, SaveRequest};
use crate::environment_picker::{CreateEnvironmentRequested, EnvironmentPicker};
use crate::save_request;
use collections_panel_ui::CollectionPanel;
use request_eagle_theme::method_color;
use tab_ui::{
    CollectionPage, CookiePage, EnvironmentEditor, Environments, EnvironmentsEvent, GrpcDraft,
    RequestDraft, RequestLocation, SaveCollection, WebSocketDraft,
};

// Rendering and virtualization share the same relative geometry at every zoom.
const TAB_WIDTH: Rems = rems(12.);
const TAB_HEIGHT: Rems = rems(2.);

/// The content of a tab. Each tab owns its page entity, preserving page state
/// when switching tabs.
#[derive(Clone)]
pub(crate) enum Page {
    Request(Entity<RequestDraft>),
    Grpc(Entity<GrpcDraft>),
    WebSocket(Entity<WebSocketDraft>),
    Collection(Entity<CollectionPage>),
    Environment(Entity<EnvironmentEditor>),
    Cookies(Entity<CookiePage>),
}

impl Page {
    fn is_dirty(&self, cx: &App) -> bool {
        match self {
            Page::Request(draft) => draft.read(cx).is_dirty(),
            Page::Grpc(draft) => draft.read(cx).is_dirty(),
            Page::WebSocket(draft) => draft.read(cx).is_dirty(),
            Page::Collection(page) => page.read(cx).is_dirty(),
            Page::Environment(editor) => editor.read(cx).is_dirty(),
            Page::Cookies(_) => false,
        }
    }

    /// The method or protocol shown before a request tab's title.
    fn label(&self, cx: &App) -> Option<&'static str> {
        match self {
            Page::Request(draft) => Some(draft.read(cx).request.method.as_str()),
            Page::Grpc(_) => Some("gRPC"),
            Page::WebSocket(_) => Some("WS"),
            Page::Collection(_) | Page::Environment(_) | Page::Cookies(_) => None,
        }
    }

    /// Where a request tab's request is saved.
    pub(crate) fn location<'a>(&self, cx: &'a App) -> Option<&'a RequestLocation> {
        match self {
            Page::Request(draft) => draft.read(cx).location.as_ref(),
            Page::Grpc(draft) => draft.read(cx).location.as_ref(),
            Page::WebSocket(draft) => draft.read(cx).location.as_ref(),
            Page::Collection(_) | Page::Environment(_) | Page::Cookies(_) => None,
        }
    }

    fn icon(&self) -> Option<&'static str> {
        match self {
            Page::Request(_) | Page::Grpc(_) | Page::WebSocket(_) => None,
            Page::Collection(_) => Some("icons/package.svg"),
            Page::Environment(_) => Some("icons/globe.svg"),
            Page::Cookies(_) => Some("icons/cookie.svg"),
        }
    }

    /// Redraw the tab strip only when the tab's label or dirty marker changes.
    fn observe(&self, id: u64, cx: &mut Context<MainView>) -> Subscription {
        let on_change = move |this: &mut MainView, cx: &mut Context<MainView>| {
            let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            let label = tab.page.label(cx);
            let dirty = tab.page.is_dirty(cx);

            if tab.label != label || tab.dirty != dirty {
                tab.label = label;
                tab.dirty = dirty;
                cx.notify();
            }
        };

        match self {
            Page::Request(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::Grpc(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::WebSocket(draft) => cx.observe(draft, move |this, _, cx| on_change(this, cx)),
            Page::Collection(page) => cx.observe(page, move |this, _, cx| on_change(this, cx)),
            Page::Environment(editor) => cx.observe(editor, move |this, _, cx| on_change(this, cx)),
            Page::Cookies(page) => cx.observe(page, move |this, _, cx| on_change(this, cx)),
        }
    }

    fn prepare(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Request(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::Grpc(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::WebSocket(draft) => draft.update(cx, |draft, cx| draft.prepare(window, cx)),
            Page::Collection(page) => page.update(cx, |page, cx| page.prepare(window, cx)),
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
            Page::Collection(page) => page
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
    _subscriptions: Vec<Subscription>,
}

pub(crate) struct MainView {
    pub(crate) tabs: Vec<PageTab>,
    pub(crate) selected: Option<usize>,
    next_id: u64,
    scroll: ScrollHandle,
    scroll_to_tab: Option<usize>,
    focus: FocusHandle,
    pending_close: Option<u64>,
    save_error: Option<String>,
    variable_sessions: environment::EnvironmentSessions,
    pub(crate) environments: Entity<Environments>,
    environment_picker: Entity<EnvironmentPicker>,
    /// Stores saved requests and collections.
    sidebar: Entity<CollectionPanel>,
    _environment_subscriptions: [Subscription; 2],
}

impl MainView {
    pub(crate) fn new(
        environments: Entity<Environments>,
        sidebar: Entity<CollectionPanel>,
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

        let mut view = Self {
            tabs: Vec::new(),
            selected: None,
            next_id: 1,
            scroll: ScrollHandle::new(),
            scroll_to_tab: None,
            focus: cx.focus_handle(),
            pending_close: None,
            save_error: None,
            variable_sessions: environment::EnvironmentSessions::default(),
            environments,
            environment_picker,
            sidebar,
            _environment_subscriptions: [picker_subscription, environments_subscription],
        };

        view.new_tab(cx);

        view
    }

    fn open_tab(
        &mut self,
        title: impl Into<SharedString>,
        page: Page,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_id;
        let subscription = page.observe(id, cx);

        self.tabs.push(PageTab {
            id,
            title: title.into(),
            label: page.label(cx),
            dirty: page.is_dirty(cx),
            page,
            _subscriptions: vec![subscription],
        });
        self.next_id += 1;

        let index = self.tabs.len() - 1;
        self.select_tab(index, cx);

        index
    }

    /// Show a saved request, reusing its tab when it is open.
    pub(crate) fn open_request(
        &mut self,
        location: RequestLocation,
        request: &request::Request,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.request_tab(&location.path, &location.id, cx) {
            self.set_request_location(index, location, cx);
            self.select_tab(index, cx);
            return;
        }

        match request {
            request::Request::Http(request) => {
                self.open_draft(location.name.clone(), request.clone(), Some(location), cx)
            }
            request::Request::Grpc(request) => {
                self.open_grpc_draft(location.name.clone(), request.clone(), Some(location), cx)
            }
            request::Request::WebSocket(request) => {
                self.open_websocket(location.name.clone(), request.clone(), Some(location), cx)
            }
        }
    }

    /// Follow a request renamed or moved in the sidebar.
    pub(crate) fn relocate_request(
        &mut self,
        previous_path: &Path,
        location: RequestLocation,
        cx: &mut Context<Self>,
    ) {
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
        location: RequestLocation,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[index];
        tab.title = location.name.clone();

        match &tab.page {
            Page::Request(draft) => draft.update(cx, |draft, cx| draft.set_location(location, cx)),
            Page::Grpc(draft) => draft.update(cx, |draft, cx| draft.set_location(location, cx)),
            Page::WebSocket(draft) => {
                draft.update(cx, |draft, cx| draft.set_location(location, cx))
            }
            Page::Collection(_) | Page::Environment(_) | Page::Cookies(_) => {}
        }
    }

    pub(crate) fn open_collection(
        &mut self,
        path: &Path,
        name: SharedString,
        variables: HashMap<String, String>,
        scripts: request::RequestScripts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.collection_tab(path, cx) {
            self.select_tab(index, cx);
            return;
        }

        let page = cx
            .new(|_| CollectionPage::new(path.to_path_buf(), name.to_string(), variables, scripts));
        let index = self.open_tab(name, Page::Collection(page.clone()), cx);
        let id = self.tabs[index].id;
        let subscription = cx.subscribe_in(
            &page,
            window,
            move |this, _, _: &SaveCollection, window, cx| {
                if let Some(index) = this.tabs.iter().position(|tab| tab.id == id) {
                    this.save_tab(index, window, cx);
                }
            },
        );
        self.tabs[index]._subscriptions.push(subscription);
    }

    fn collection_tab(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| match &tab.page {
            Page::Collection(page) => page.read(cx).path == path,
            _ => false,
        })
    }

    /// Close a deleted collection's tab, so a later collection at the same
    /// path cannot reuse its stale settings.
    pub(crate) fn close_collection(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(index) = self.collection_tab(path, cx) {
            self.remove_tab(index, cx);
        }
    }

    /// Follow a collection renamed in the sidebar.
    pub(crate) fn relocate_collection(
        &mut self,
        previous_path: &Path,
        path: &Path,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.collection_tab(previous_path, cx) else {
            return;
        };
        let tab = &mut self.tabs[index];
        tab.title = name.clone();

        if let Page::Collection(page) = &tab.page {
            page.update(cx, |page, cx| {
                page.relocate(path.to_path_buf(), name.to_string(), window, cx)
            });
        }
        cx.notify();
    }

    pub(crate) fn new_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_draft(title.into(), Default::default(), None, cx);
    }

    pub(crate) fn new_websocket_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_websocket(title.into(), Default::default(), None, cx);
    }

    fn open_websocket(
        &mut self,
        title: SharedString,
        request: request::WebSocketRequest,
        location: Option<RequestLocation>,
        cx: &mut Context<Self>,
    ) {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft =
            cx.new(|cx| WebSocketDraft::new(request, location, sessions, Some(environments), cx));

        self.open_tab(title, Page::WebSocket(draft), cx);
    }

    fn open_draft(
        &mut self,
        title: SharedString,
        request: request::HttpRequest,
        location: Option<RequestLocation>,
        cx: &mut Context<Self>,
    ) {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft =
            cx.new(|cx| RequestDraft::new(request, location, sessions, Some(environments), cx));

        self.open_tab(title, Page::Request(draft), cx);
    }

    pub(crate) fn new_grpc_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_grpc_draft(title.into(), Default::default(), None, cx);
    }

    fn open_grpc_draft(
        &mut self,
        title: SharedString,
        request: request::GrpcRequest,
        location: Option<RequestLocation>,
        cx: &mut Context<Self>,
    ) {
        let sessions = self.variable_sessions.clone();
        let environments = self.environments.clone();
        let draft =
            cx.new(|cx| GrpcDraft::new(request, location, sessions, Some(environments), cx));

        self.open_tab(title, Page::Grpc(draft), cx);
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
            EnvironmentsEvent::Renamed { to, .. } => {
                // Only the environment's editor renames it, and it already
                // uses the new name.
                if let Some((index, _)) = self.environment_tab(to, cx) {
                    self.tabs[index].title = to.clone();
                }
            }
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
            self.save_tab(index, window, cx);
        }
    }

    fn save_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let id = tab.id;
        self.save_error = None;

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
                    self.sidebar
                        .update(cx, |sidebar, cx| {
                            sidebar.save_collection(
                                &path,
                                &settings.name,
                                settings.variables.iter().cloned().collect(),
                                settings.scripts.clone(),
                                cx,
                            )
                        })
                        .map(|path| (path, settings))
                        .map_err(|error| error.to_string())
                });

                match result {
                    Ok((path, settings)) => {
                        self.tabs[index].title = settings.name.clone().into();
                        page.update(cx, |page, cx| page.mark_saved(path, settings, cx));
                        self.close_saved_tab(index, window, cx);
                    }
                    Err(error) => {
                        self.save_error = Some(format!("Could not save collection: {error}"))
                    }
                }
            }
            // The jar saves itself whenever it changes.
            Page::Cookies(_) => {}
            Page::Request(draft) => {
                let request = draft.read(cx).request.clone();
                let location = draft.read(cx).location.clone();

                if self.save_request_at(id, location, request.clone().into(), window, cx) {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
            Page::Grpc(draft) => {
                let request = draft.read(cx).request.clone();
                let location = draft.read(cx).location.clone();

                if self.save_request_at(id, location, request.clone().into(), window, cx) {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
            Page::WebSocket(draft) => {
                let request = draft.read(cx).request.clone();
                let location = draft.read(cx).location.clone();

                if self.save_request_at(id, location, request.clone().into(), window, cx) {
                    draft.update(cx, |draft, cx| draft.mark_saved(request, cx));
                    self.close_saved_tab(index, window, cx);
                }
            }
        }

        cx.notify();
    }

    /// Save a request tab to its file. An unsaved request opens the dialog
    /// that chooses where; that dialog marks the tab saved itself.
    fn save_request_at(
        &mut self,
        tab_id: u64,
        location: Option<RequestLocation>,
        request: request::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(location) = location else {
            save_request::open(
                cx.entity(),
                self.sidebar.clone(),
                tab_id,
                request,
                window,
                cx,
            );
            return false;
        };

        let result = self.sidebar.update(cx, |sidebar, cx| {
            sidebar.save_request(&location.path, &location.id, request, cx)
        });

        if let Err(error) = &result {
            self.save_error = Some(format!("Could not save request: {error}"));
        }

        result.is_ok()
    }

    /// Attach a new request draft to the file it was saved as.
    pub(crate) fn attach_saved_request(
        &mut self,
        tab_id: u64,
        file: &collection::FileEntry,
        destination: &collections_panel_ui::SaveDestination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let location = RequestLocation {
            path: file.path.clone(),
            id: file.id.clone().into(),
            name: file.name.clone().into(),
            collection: destination.collection.clone(),
            folders: destination.folders.clone(),
        };
        self.set_request_location(index, location, cx);

        match (&self.tabs[index].page, &file.request) {
            (Page::Request(draft), request::Request::Http(request)) => {
                draft.update(cx, |draft, cx| draft.mark_saved(request.clone(), cx));
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
            .dropdown_menu(move |menu, _, _| {
                let http_view = view.clone();
                let grpc_view = view.clone();
                let websocket_view = view.clone();

                menu.action_context(focus.clone())
                    .item(
                        PopupMenuItem::new("HTTP Request")
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
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(method_color(label, cx))
                        .child(label),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(tab.title.clone()),
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
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_tab(index, cx);
                this.focus(window, cx);
            }))
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "main-view".into())
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .debug_selector(|| "main-tab-bar".into())
                    .flex_none()
                    .h_10()
                    .px_1()
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
            .when(self.pending_close.is_some(), |this| {
                this.child(self.close_confirmation(cx))
            })
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
