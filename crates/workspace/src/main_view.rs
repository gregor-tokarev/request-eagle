use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{button::*, *};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::environment_picker::{EnvironmentPicker, EnvironmentPickerEvent};
use crate::actions::{CloseTab, NewTab, SaveRequest};
use tab_ui::{
    CollectionPage, CollectionSettings, EnvironmentEditor, Environments, EnvironmentsEvent,
    RequestDraft, SaveCollection, TabBadge, TabBadgeTone, TabPage, TabView,
};

// Rendering and virtualization share the same relative geometry at every zoom.
const TAB_WIDTH: Rems = rems(12.);
const TAB_HEIGHT: Rems = rems(2.);

pub(crate) struct PageTab {
    pub(crate) id: u64,
    pub(crate) title: SharedString,
    pub(crate) request_path: Option<PathBuf>,
    pub(crate) request_id: Option<SharedString>,
    pub(crate) badge: Option<TabBadge>,
    icon: Option<&'static str>,
    dirty: bool,
    pub(crate) page: TabView,
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
    _environment_subscriptions: [Subscription; 2],
}

pub(crate) struct RequestSaveRequested {
    pub(crate) tab_id: u64,
    pub(crate) path: PathBuf,
    pub(crate) request_id: SharedString,
    pub(crate) request: collection::HttpRequest,
}

pub(crate) struct NewRequestSaveRequested {
    pub(crate) tab_id: u64,
    pub(crate) request: collection::HttpRequest,
}

pub(crate) struct CollectionSaveRequested {
    pub(crate) tab_id: u64,
    pub(crate) path: PathBuf,
    pub(crate) settings: CollectionSettings,
}

impl EventEmitter<RequestSaveRequested> for MainView {}
impl EventEmitter<NewRequestSaveRequested> for MainView {}
impl EventEmitter<CollectionSaveRequested> for MainView {}

impl MainView {
    pub(crate) fn new(
        environments: Entity<Environments>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let environment_picker =
            cx.new(|cx| EnvironmentPicker::new(environments.clone(), window, cx));
        let picker_subscription = cx.subscribe_in(
            &environment_picker,
            window,
            |this, _, event: &EnvironmentPickerEvent, window, cx| match event {
                EnvironmentPickerEvent::Open(name) => {
                    this.open_environment(name.clone(), window, cx);
                }
                EnvironmentPickerEvent::Create => this.create_environment(window, cx),
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
            _environment_subscriptions: [picker_subscription, environments_subscription],
        };

        view.new_tab(cx);

        view
    }

    /// Each tab owns its page entity, preserving page state when switching tabs.
    pub(crate) fn open_tab<T: TabPage>(
        &mut self,
        title: impl Into<SharedString>,
        page: Entity<T>,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_id;
        let state = page.read(cx).tab_state();
        let subscription = cx.observe(&page, move |this, page, cx| {
            let state = page.read(cx).tab_state();

            if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id)
                && (tab.badge != state.badge || tab.dirty != state.dirty)
            {
                tab.badge = state.badge;
                tab.dirty = state.dirty;
                cx.notify();
            }
        });

        self.tabs.push(PageTab {
            id,
            title: title.into(),
            request_path: None,
            request_id: None,
            badge: state.badge,
            icon: state.icon,
            dirty: state.dirty,
            page: TabView::new(page),
            _subscriptions: vec![subscription],
        });
        self.next_id += 1;

        let index = self.tabs.len() - 1;
        self.select_tab(index, cx);

        index
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The parameters mirror CollectionPanelEvent::OpenRequest; the workspace owns tab state, not sidebar events."
    )]
    pub(crate) fn open_request(
        &mut self,
        path: &Path,
        request_id: SharedString,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
        request: &collection::Request,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.request_path.as_deref() == Some(path)
                && tab.request_id.as_ref() == Some(&request_id)
        }) {
            self.tabs[index].title = name.clone();
            if let Ok(draft) = self.tabs[index].page.view().downcast::<RequestDraft>() {
                draft.update(cx, |draft, cx| {
                    draft.name = name;
                    draft.collection = Some(collection);
                    draft.set_variable_environment(path, folders.len(), cx);
                    draft.folders = folders;
                    cx.notify();
                });
            }
            self.select_tab(index, cx);

            return;
        }

        let collection::Request::Http(request) = request;
        let mut draft = RequestDraft::from_saved(name.clone(), collection, request.clone());
        draft.set_variable_environment(path, folders.len(), cx);
        draft.folders = folders;
        let index = self.open_draft(name, draft, cx);
        self.tabs[index].request_path = Some(path.to_path_buf());
        self.tabs[index].request_id = Some(request_id);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The parameters mirror CollectionPanelEvent::RequestRelocated while keeping the tab view independent of the sidebar."
    )]
    pub(crate) fn relocate_request(
        &mut self,
        previous_path: &Path,
        path: &Path,
        request_id: &SharedString,
        name: SharedString,
        collection: SharedString,
        folders: Vec<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| {
            tab.request_path.as_deref() == Some(previous_path)
                && tab.request_id.as_ref() == Some(request_id)
        }) {
            tab.request_path = Some(path.to_path_buf());
            tab.title = name.clone();

            if let Ok(draft) = tab.page.view().downcast::<RequestDraft>() {
                draft.update(cx, |draft, cx| {
                    draft.name = name;
                    draft.collection = Some(collection);
                    draft.set_variable_environment(path, folders.len(), cx);
                    draft.folders = folders;
                    cx.notify();
                });
            }
            cx.notify();
        }
    }

    pub(crate) fn open_collection(
        &mut self,
        path: &Path,
        name: SharedString,
        variables: HashMap<String, String>,
        scripts: collection::RequestScripts,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.collection_tab(path, cx) {
            self.select_tab(index, cx);
            return;
        }

        let page = cx
            .new(|_| CollectionPage::new(path.to_path_buf(), name.to_string(), variables, scripts));
        let index = self.open_tab(name, page.clone(), cx);
        let id = self.tabs[index].id;
        let subscription = cx.subscribe(&page, move |this, _, _: &SaveCollection, cx| {
            if let Some(index) = this.tabs.iter().position(|tab| tab.id == id) {
                this.save_tab(index, cx);
            }
        });
        self.tabs[index]._subscriptions.push(subscription);
    }

    fn collection_tab(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.page
                .view()
                .downcast::<CollectionPage>()
                .is_ok_and(|page| page.read(cx).path == path)
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

        if let Ok(page) = tab.page.view().downcast::<CollectionPage>() {
            page.update(cx, |page, cx| {
                page.relocate(path.to_path_buf(), name.to_string(), window, cx)
            });
        }
        cx.notify();
    }

    pub(crate) fn new_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        self.open_draft(title.into(), RequestDraft::new(), cx);
    }

    fn open_draft(
        &mut self,
        title: SharedString,
        mut draft: RequestDraft,
        cx: &mut Context<Self>,
    ) -> usize {
        draft.set_variable_sessions(self.variable_sessions.clone(), cx);
        draft.set_environments(self.environments.clone(), cx);
        let page = cx.new(|_| draft);
        self.open_tab(title, page, cx)
    }

    fn environment_tab(&self, name: &str, cx: &App) -> Option<(usize, Entity<EnvironmentEditor>)> {
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            let editor = tab.page.view().downcast::<EnvironmentEditor>().ok()?;
            (editor.read(cx).name == name).then_some((index, editor))
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
            self.open_tab(name, editor.clone(), cx);
            editor
        };

        self.focus(window, cx);
        editor
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
            EnvironmentsEvent::Renamed { from, to } => {
                // The editor that renamed the environment may already use the new name.
                for tab in &mut self.tabs {
                    if let Ok(editor) = tab.page.view().downcast::<EnvironmentEditor>()
                        && [from, to].contains(&&editor.read(cx).name)
                    {
                        editor.update(cx, |editor, _| editor.name = to.clone());
                        tab.title = to.clone();
                    }
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

        if self.tabs[index].page.state(cx).dirty {
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

    pub(crate) fn save_active_request(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.save_tab(index, cx);
        }
    }

    fn save_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };

        if let Ok(editor) = tab.page.view().downcast::<EnvironmentEditor>() {
            let id = tab.id;
            // The editor shows its own save errors next to the variables.
            let saved = editor.update(cx, |editor, cx| editor.save(cx)).is_ok();
            self.tabs[index].dirty = editor.read(cx).is_dirty();
            self.save_error = None;

            if saved && self.pending_close == Some(id) {
                self.remove_tab(index, cx);
            }

            cx.notify();
            return;
        }

        if let Ok(page) = tab.page.view().downcast::<CollectionPage>() {
            let page = page.read(cx);
            match page.settings() {
                Ok(settings) => {
                    self.save_error = None;
                    cx.emit(CollectionSaveRequested {
                        tab_id: tab.id,
                        path: page.path.clone(),
                        settings,
                    });
                }
                Err(error) => self.save_error = Some(format!("Could not save collection: {error}")),
            }
            cx.notify();
            return;
        }

        let Ok(draft) = tab.page.view().downcast::<RequestDraft>() else {
            return;
        };
        let (Some(path), Some(request_id)) = (tab.request_path.clone(), tab.request_id.clone())
        else {
            self.save_error = None;
            cx.emit(NewRequestSaveRequested {
                tab_id: tab.id,
                request: draft.read(cx).request.clone(),
            });
            cx.notify();
            return;
        };

        self.save_error = None;
        cx.emit(RequestSaveRequested {
            tab_id: tab.id,
            path,
            request_id,
            request: draft.read(cx).request.clone(),
        });
        cx.notify();
    }

    pub(crate) fn attach_saved_request(
        &mut self,
        tab_id: u64,
        file: &collection::FileEntry,
        destination: &collections_panel_ui::SaveDestination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        tab.request_path = Some(file.path.clone());
        tab.request_id = Some(file.id.clone().into());
        tab.title = file.name.clone().into();
        if let Ok(draft) = tab.page.view().downcast::<RequestDraft>() {
            draft.update(cx, |draft, cx| {
                draft.name = file.name.clone().into();
                draft.collection = Some(destination.collection.clone());
                draft.set_variable_environment(&file.path, destination.folders.len(), cx);
                draft.folders = destination.folders.clone();
                cx.notify();
            });
        }
        let collection::Request::Http(request) = &file.request;
        self.finish_save(
            &RequestSaveRequested {
                tab_id,
                path: file.path.clone(),
                request_id: file.id.clone().into(),
                request: request.clone(),
            },
            Ok(()),
            window,
            cx,
        );
    }

    pub(crate) fn finish_save(
        &mut self,
        event: &RequestSaveRequested,
        result: Result<(), collection::CollectionEditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == event.tab_id) else {
            return;
        };

        match result {
            Ok(()) => {
                if let Ok(draft) = self.tabs[index].page.view().downcast::<RequestDraft>() {
                    draft.update(cx, |draft, cx| draft.mark_saved(event.request.clone(), cx));
                    self.tabs[index].dirty = draft.read(cx).is_dirty();
                }
                self.save_error = None;

                if self.pending_close == Some(event.tab_id) && !self.tabs[index].dirty {
                    self.remove_tab(index, cx);
                    self.focus(window, cx);
                }
            }
            Err(error) => self.save_error = Some(format!("Could not save request: {error}")),
        }

        cx.notify();
    }

    pub(crate) fn finish_collection_save(
        &mut self,
        event: &CollectionSaveRequested,
        result: Result<PathBuf, collection::CollectionEditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == event.tab_id) else {
            return;
        };

        match result {
            Ok(path) => {
                let tab = &mut self.tabs[index];
                tab.title = event.settings.name.clone().into();
                if let Ok(page) = tab.page.view().downcast::<CollectionPage>() {
                    page.update(cx, |page, cx| {
                        page.mark_saved(path, event.settings.clone(), cx)
                    });
                    tab.dirty = page.read(cx).is_dirty();
                }
                self.save_error = None;

                if self.pending_close == Some(event.tab_id) && !self.tabs[index].dirty {
                    self.remove_tab(index, cx);
                    self.focus(window, cx);
                }
            }
            Err(error) => self.save_error = Some(format!("Could not save collection: {error}")),
        }

        cx.notify();
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
                    .on_click(cx.listener(|this, _, _, cx| this.save_active_request(cx))),
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

    pub(crate) fn send_request(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.tabs[index].page.send(window, cx);
        }
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
            .when_some(tab.icon, |this, icon| {
                this.child(
                    Icon::default()
                        .path(icon)
                        .size(rems(0.875))
                        .flex_none()
                        .text_color(cx.theme().muted_foreground),
                )
            })
            .when_some(tab.badge, |this, badge| {
                let color = match badge.tone {
                    TabBadgeTone::Success => cx.theme().success,
                    TabBadgeTone::Warning => cx.theme().warning,
                    TabBadgeTone::Info => cx.theme().info,
                    TabBadgeTone::Danger => cx.theme().danger,
                };

                this.child(
                    div()
                        .debug_selector(move || format!("tab-method-{id}"))
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(badge.label),
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
