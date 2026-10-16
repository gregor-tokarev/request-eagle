use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::actions::*;
use crate::{
    bottom_panel::BottomPanel,
    command_palette::CommandPalette,
    environment_panel::{EnvironmentPanel, EnvironmentPanelEvent},
    flow_panel::{FlowPanel, FlowPanelEvent},
    git_panel::GitPanel,
    git_state::{GitAction, GitEvent, GitState},
    history_panel::{HistoryPanel, HistoryPanelEvent},
    main_view::{MainView, Page, TabEdited},
    session::{SavedSidebar, SavedWindow, Session, SessionWriter},
    top_panel::TopPanel,
};
use agent_panel::{AgentPanel, AgentPanelEvent, OpenAgentSettings, ToggleAgentPanel};
use collection::{CollectionRegistry, Collections};
use collections_panel_ui::{CollectionPanel, CollectionPanelEvent};
use environment::GlobalEnvironments;
use flow::{FlowLibrary, SavedFlow};
use gpui_kit::base::motion::{self, Transition};
use gpui_kit::component::{
    animation::ease_in_out_cubic,
    button::{Button, ButtonVariants},
    notification::Notification,
    resizable::{ResizableState, h_resizable, resizable_panel},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_history::History;
use settings_ui::{Settings, SettingsEvent, SettingsPage};
use tab_ui::{AddToAgent, Cite, CiteToAgent, Environments};
use updater::Updater;

/// How long after a tab changes its session is saved. Changes that follow
/// within the delay are saved together.
const CHECKPOINT_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarSection {
    Collections,
    Environments,
    Flows,
    History,
}

pub(crate) struct Workspace {
    top_panel: Entity<TopPanel>,
    pub(crate) sidebar: Entity<CollectionPanel>,
    pub(crate) environment_panel: Entity<EnvironmentPanel>,
    flow_panel: Entity<FlowPanel>,
    history: Entity<HistoryPanel>,
    collections_open: bool,
    environments_open: bool,
    flows_open: bool,
    history_open: bool,
    pub(crate) collections_header: FocusHandle,
    environments_header: FocusHandle,
    flows_header: FocusHandle,
    history_header: FocusHandle,
    pub(crate) main_view: Entity<MainView>,
    bottom_panel: Entity<BottomPanel>,
    git: Entity<GitState>,
    git_panel: Entity<GitPanel>,
    collections: Entity<Collections>,
    agent_panel: Entity<AgentPanel>,
    agent_panel_visible: Entity<bool>,

    pub(crate) main_split: Entity<ResizableState>,
    pub(crate) sidebar_visible: Entity<bool>,

    /// Built when first opened.
    pub(crate) settings: Option<Entity<Settings>>,
    pub(crate) settings_visible: bool,
    previous_focus: Option<FocusHandle>,
    updater: Entity<Updater>,

    pub(crate) command_palette: Option<WeakEntity<list::ListState<CommandPalette>>>,

    /// Where the window, sidebar and tabs are saved, shortly after tabs
    /// change and when the window closes.
    session_path: PathBuf,
    session_writer: Arc<SessionWriter>,
    /// Counts the sessions captured, so an earlier one never replaces a
    /// later one.
    sessions_captured: u64,
    /// Whether the session is about to be saved after a change to the tabs.
    checkpoint_scheduled: bool,

    _sidebar_subscription: Subscription,
    _environment_panel_subscription: Subscription,
    _flow_panel_subscription: Subscription,
    _history_subscription: Subscription,
    _tabs_subscriptions: [Subscription; 2],
    _git_subscriptions: [Subscription; 3],
    _agent_panel_subscription: Subscription,
    _settings_subscription: Option<Subscription>,
}

impl Workspace {
    // Each store is loaded once at startup and handed over here.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        collections: CollectionRegistry,
        environments: GlobalEnvironments,
        flows: FlowLibrary,
        history: History,
        updater: Entity<Updater>,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Session {
            path: session_path,
            sidebar: sidebar_state,
            tabs,
            selected_tab,
            agent_panel: agent_panel_open,
            ..
        } = session;
        let sidebar_visible = cx.new(|_| sidebar_state.visible);
        let collections = cx.new(|_| Collections::new(collections));

        let git = cx.new(|cx| GitState::new(collections.clone(), cx));
        let git_panel = cx.new(|cx| GitPanel::new(git.clone(), sidebar_state.git, window, cx));
        let agent_panel_visible = cx.new(|_| agent_panel_open);
        // The bottom panel is cached, so it observes what it shows itself.
        let bottom_panel = cx.new(|cx| {
            BottomPanel::new(
                sidebar_visible.clone(),
                agent_panel_visible.clone(),
                git.clone(),
                git_panel.clone(),
                cx,
            )
        });

        // The session file is in the app's data directory.
        let data_directory = session_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let agent_panel = cx.new(|cx| AgentPanel::new(data_directory, window, cx));
        let agent_panel_subscription = cx.subscribe_in(
            &agent_panel,
            window,
            |this, _, event: &AgentPanelEvent, window, cx| match event {
                AgentPanelEvent::FilesChanged => this.reload_saved(cx),
                AgentPanelEvent::ShowCite(origin) => this
                    .main_view
                    .update(cx, |view, cx| view.show_cite(origin, window, cx)),
            },
        );

        let sidebar = cx.new(|cx| {
            CollectionPanel::new(collections.clone(), sidebar_state.collapsed, window, cx)
        });
        let sidebar_subscription =
            cx.subscribe_in(&sidebar, window, |this, _, event, window, cx| match event {
                CollectionPanelEvent::Open(path) => {
                    this.main_view.update(cx, |view, cx| {
                        view.open_saved(path, window, cx);
                        view.prepare_active_tab(window, cx);
                    });
                }
                CollectionPanelEvent::Run(path) => {
                    this.main_view.update(cx, |view, cx| {
                        view.open_runner(path, cx);
                        view.prepare_active_tab(window, cx);
                    });
                }
                CollectionPanelEvent::OpenUnsavedRequest(request) => {
                    this.update_tabs(window, cx, |view, cx| {
                        view.open_unsaved_request(request.clone(), cx)
                    });
                }
                CollectionPanelEvent::EnvironmentsImported => {
                    // Show the imported environments in the sidebar.
                    this.environments_open = true;
                    let environments = this.main_view.read(cx).environments.clone();
                    environments.update(cx, |environments, cx| environments.refresh(cx));
                    cx.notify();
                }
            });

        let active_environment = cx
            .try_global::<preferences::Preferences>()
            .and_then(|preferences| preferences.active_environment.clone());
        let environments = cx.new(|_| Environments::new(environments, active_environment));
        let environment_panel = cx.new(|cx| EnvironmentPanel::new(environments.clone(), cx));
        let environment_panel_subscription = cx.subscribe_in(
            &environment_panel,
            window,
            |this, _, event: &EnvironmentPanelEvent, window, cx| {
                this.main_view.update(cx, |view, cx| match event {
                    EnvironmentPanelEvent::Open(name) => {
                        view.open_environment(name.clone(), window, cx);
                    }
                    EnvironmentPanelEvent::Rename(name) => {
                        view.rename_environment(name.clone(), window, cx)
                    }
                });
            },
        );

        let flow_panel = cx.new(|cx| FlowPanel::new(flows, cx));
        let flow_panel_subscription = cx.subscribe_in(
            &flow_panel,
            window,
            |this, _, event: &FlowPanelEvent, window, cx| match event {
                FlowPanelEvent::Open(saved) => this.open_flow(saved, window, cx),
                FlowPanelEvent::Renamed { path, name } => {
                    this.main_view
                        .update(cx, |view, cx| view.rename_flow(path, name.clone(), cx));
                }
                FlowPanelEvent::Deleted { path } => {
                    this.main_view
                        .update(cx, |view, cx| view.close_flow(path, cx));
                }
                FlowPanelEvent::Refreshed => {
                    this.main_view
                        .update(cx, |view, cx| view.follow_flows(window, cx));
                }
            },
        );

        let history = cx.new(|cx| HistoryPanel::new(history, window, cx));
        let history_subscription = cx.subscribe_in(
            &history,
            window,
            |this, _, event: &HistoryPanelEvent, window, cx| match event {
                HistoryPanelEvent::Open { entry, record } => {
                    this.main_view.update(cx, |view, cx| {
                        view.open_history(entry, record.clone(), window, cx);
                        view.prepare_active_tab(window, cx);
                    });
                }
            },
        );

        let main_view = cx.new(|cx| {
            MainView::new(
                environments,
                collections.clone(),
                flow_panel.clone(),
                history.clone(),
                tabs,
                selected_tab,
                window,
                cx,
            )
        });

        // The tab strip redraws as tabs open, close and are selected; edits
        // inside a tab are reported apart, so they do not redraw it.
        let tabs_subscriptions = [
            cx.observe_in(&main_view, window, |this, _, window, cx| {
                this.checkpoint_session(window, cx);

                // The next thread works where the active tab's collection is.
                let directory = this.agent_directory(cx);
                this.agent_panel
                    .update(cx, |panel, cx| panel.set_directory(directory, cx));
            }),
            cx.subscribe_in(&main_view, window, |this, _, _: &TabEdited, window, cx| {
                this.checkpoint_session(window, cx)
            }),
        ];

        let git_subscriptions = [
            cx.subscribe_in(&git, window, |_, _, event: &GitEvent, window, cx| {
                let GitEvent::Finished { action, errors } = event;
                window.push_notification(git_notification(*action, errors), cx);
            }),
            // The sidebar counts each collection's changes.
            cx.observe(&git, |this, git, cx| {
                let changes = git.read(cx).changes_by_collection();
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.set_git_changes(changes, cx));
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.git.update(cx, |git, cx| git.refresh_on_return(cx));
                }
            }),
        ];

        // Start in the collections tree, or in the tabs while it is hidden,
        // folded or in place of the Git panel.
        if sidebar_state.visible && sidebar_state.collections && !sidebar_state.git {
            window.focus(&sidebar.focus_handle(cx), cx);
            main_view.update(cx, |view, cx| view.prepare_active_tab(window, cx));
        } else {
            main_view.update(cx, |view, cx| view.focus(window, cx));
        }

        let mut workspace = Self {
            top_panel: cx.new(|_| TopPanel),
            sidebar,
            environment_panel,
            flow_panel,
            history,
            collections_open: sidebar_state.collections,
            environments_open: sidebar_state.environments,
            flows_open: sidebar_state.flows,
            history_open: sidebar_state.history,
            collections_header: cx.focus_handle(),
            environments_header: cx.focus_handle(),
            flows_header: cx.focus_handle(),
            history_header: cx.focus_handle(),
            main_view,
            bottom_panel,
            git,
            git_panel,
            collections,
            agent_panel,
            agent_panel_visible,
            main_split: cx.new(|_| ResizableState::default()),
            sidebar_visible,
            settings: None,
            settings_visible: false,
            previous_focus: None,
            updater,
            command_palette: None,
            session_path,
            session_writer: Arc::default(),
            sessions_captured: 0,
            checkpoint_scheduled: false,
            _sidebar_subscription: sidebar_subscription,
            _environment_panel_subscription: environment_panel_subscription,
            _flow_panel_subscription: flow_panel_subscription,
            _history_subscription: history_subscription,
            _tabs_subscriptions: tabs_subscriptions,
            _git_subscriptions: git_subscriptions,
            _agent_panel_subscription: agent_panel_subscription,
            _settings_subscription: None,
        };

        if agent_panel_open {
            workspace.prepare_agent(cx);
        }

        workspace
    }

    /// Save the window, sidebar and tabs for the next launch.
    fn save_session(&mut self, window: &Window, cx: &App) {
        let (number, session) = self.capture_session(window, cx);

        self.session_writer.save(number, &session);
    }

    /// Save the session shortly after the tabs change, so a crash or a forced
    /// exit keeps what was typed until then.
    fn checkpoint_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checkpoint_scheduled {
            return;
        }
        self.checkpoint_scheduled = true;

        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(CHECKPOINT_DELAY).await;

            let Ok((number, session, writer)) = this.update_in(cx, |this, window, cx| {
                this.checkpoint_scheduled = false;
                let (number, session) = this.capture_session(window, cx);

                (number, session, this.session_writer.clone())
            }) else {
                return;
            };

            // Flushing the file to disk would hold up typing.
            cx.background_executor()
                .spawn(async move { writer.save(number, &session) })
                .await;
        })
        .detach();
    }

    /// The window, sidebar and tabs as they are now, and the session's place
    /// among those captured.
    fn capture_session(&mut self, window: &Window, cx: &App) -> (u64, Session) {
        self.sessions_captured += 1;

        let session = Session {
            path: self.session_path.clone(),
            window: SavedWindow::capture(window, cx),
            sidebar: SavedSidebar {
                visible: *self.sidebar_visible.read(cx),
                collections: self.collections_open,
                environments: self.environments_open,
                flows: self.flows_open,
                history: self.history_open,
                git: self.git_panel.read(cx).open,
                // JSON holds only UTF-8 paths. Folders with other names
                // expand again, rather than failing the whole save.
                collapsed: self
                    .sidebar
                    .read(cx)
                    .collapsed_paths()
                    .into_iter()
                    .filter(|path| path.to_str().is_some())
                    .collect(),
            },
            tabs: self.main_view.read(cx).saved_tabs(cx),
            selected_tab: self.main_view.read(cx).selected,
            agent_panel: *self.agent_panel_visible.read(cx),
        };

        (self.sessions_captured, session)
    }

    pub(crate) fn open_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Settings> {
        if !self.settings_visible {
            self.previous_focus = window.focused(cx);
            self.settings_visible = true;
        }

        // The pages list every installed font and shortcut, so launching
        // does not build them.
        let settings = match &self.settings {
            Some(settings) => settings.clone(),
            None => {
                let settings = cx.new(|cx| Settings::new(self.updater.clone(), window, cx));
                self._settings_subscription = Some(cx.subscribe_in(
                    &settings,
                    window,
                    |this, _, _: &SettingsEvent, window, cx| this.close_settings(window, cx),
                ));
                self.settings = Some(settings.clone());

                settings
            }
        };

        settings.update(cx, |settings, cx| settings.focus(window, cx));

        cx.notify();

        settings
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_visible {
            return;
        }

        self.settings_visible = false;

        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        } else {
            window.blur(cx);
        }

        cx.notify();
    }

    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .command_palette
            .as_ref()
            .is_some_and(|palette| palette.upgrade().is_some())
        {
            window.close_dialog(cx);
            return;
        }

        // Leave other dialogs, such as saving a request, uninterrupted.
        if window.has_active_dialog(cx) {
            return;
        }

        if self.settings_visible {
            self.close_settings(window, cx);

            // Commands are read from the rendered workspace. Next-frame
            // callbacks run before that frame is drawn, so open the palette
            // one frame later, once the workspace is drawn again.
            cx.on_next_frame(window, |_, window, cx| {
                cx.on_next_frame(window, |this, window, cx| {
                    this.open_command_palette(window, cx)
                });
            });
            return;
        }

        self.open_command_palette(window, cx);
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Focus can stay on a row of the hidden sidebar, where workspace
        // commands do nothing. Run them from the tabs instead.
        if !window.is_action_available(&NewTab, cx) {
            self.main_view.update(cx, |view, cx| view.focus(window, cx));
        }

        let palette =
            CommandPalette::open(self.sidebar.clone(), self.main_view.clone(), window, cx);

        self.command_palette = Some(palette.downgrade());
    }

    pub(crate) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A hidden sidebar is not drawn, so keys sent to it would miss the
        // workspace's shortcuts.
        if *self.sidebar_visible.read(cx) && self.sidebar_contains_focus(window, cx) {
            self.main_view.update(cx, |view, cx| view.focus(window, cx));
        }

        self.sidebar_visible.update(cx, |visible, cx| {
            *visible = !*visible;

            cx.notify();
        });

        cx.notify();
    }

    fn sidebar_contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.git_panel.read(cx).focus.contains_focused(window, cx)
            || [
                &self.collections_header,
                &self.environments_header,
                &self.flows_header,
                &self.history_header,
            ]
            .iter()
            .any(|header| header.is_focused(window))
            || self.sidebar.read(cx).contains_focus(window, cx)
            || self
                .environment_panel
                .focus_handle(cx)
                .contains_focused(window, cx)
            || self.flow_panel.read(cx).contains_focus(window, cx)
            || self.history.read(cx).contains_focus(window, cx)
    }

    pub(crate) fn toggle_sidebar_section(
        &mut self,
        section: SidebarSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match section {
            SidebarSection::Collections => self.collections_open = !self.collections_open,
            SidebarSection::Environments => self.environments_open = !self.environments_open,
            SidebarSection::Flows => self.flows_open = !self.flows_open,
            SidebarSection::History => self.history_open = !self.history_open,
        }

        self.release_section_focus(section, window, cx);

        cx.notify();
    }

    /// Keys must not go to the rows of a folded section. Its header can open
    /// it again.
    fn release_section_focus(&self, section: SidebarSection, window: &mut Window, cx: &mut App) {
        let (open, contains_focus, header) = match section {
            SidebarSection::Collections => (
                self.collections_open,
                self.sidebar.read(cx).contains_focus(window, cx),
                &self.collections_header,
            ),
            SidebarSection::Environments => (
                self.environments_open,
                self.environment_panel
                    .focus_handle(cx)
                    .contains_focused(window, cx),
                &self.environments_header,
            ),
            SidebarSection::Flows => (
                self.flows_open,
                self.flow_panel.read(cx).contains_focus(window, cx),
                &self.flows_header,
            ),
            SidebarSection::History => (
                self.history_open,
                self.history.read(cx).contains_focus(window, cx),
                &self.history_header,
            ),
        };

        if !open && contains_focus {
            window.focus(header, cx);
        }
    }

    /// Show the collections tree, even when a shortcut runs with the sidebar
    /// hidden.
    fn reveal_collections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible.update(cx, |visible, cx| {
            *visible = true;
            cx.notify();
        });
        self.leave_git_panel(window, cx);
        self.collections_open = true;
        cx.notify();
    }

    /// Show the sidebar's sections again in place of the Git panel, taking
    /// focus out of it first, so that a dialog closing later does not give
    /// focus back to a field no longer drawn.
    fn leave_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.git_panel.read(cx).open {
            self.show_git_panel(false, window, cx);
        }
    }

    pub(crate) fn toggle_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = *self.sidebar_visible.read(cx) && self.git_panel.read(cx).open;

        self.show_git_panel(!shown, window, cx);
    }

    /// Show the Git panel in the sidebar, or the sidebar's sections again.
    fn show_git_panel(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Keys must not go to the rows of the view that leaves the sidebar.
        let focused = self.sidebar_contains_focus(window, cx);

        if open {
            self.sidebar_visible.update(cx, |visible, cx| {
                *visible = true;
                cx.notify();
            });
        }
        self.git_panel
            .update(cx, |panel, cx| panel.set_open(open, cx));

        if focused {
            if open {
                let focus = self.git_panel.read(cx).focus.clone();
                window.focus(&focus, cx);
            } else if self.collections_open {
                window.focus(&self.sidebar.focus_handle(cx), cx);
            } else {
                self.main_view.update(cx, |view, cx| view.focus(window, cx));
            }
        }

        cx.notify();
    }

    pub(crate) fn toggle_agent_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if *self.agent_panel_visible.read(cx) {
            // A hidden panel is not drawn, so keys sent to it would miss the
            // workspace's shortcuts.
            if self
                .agent_panel
                .focus_handle(cx)
                .contains_focused(window, cx)
            {
                self.main_view.update(cx, |view, cx| view.focus(window, cx));
            }
            self.agent_panel_visible.update(cx, |visible, cx| {
                *visible = false;
                cx.notify();
            });
            cx.notify();
            return;
        }

        self.show_agent_panel(window, cx);
    }

    /// Show the agent panel with its message box focused.
    fn show_agent_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = *self.agent_panel_visible.read(cx);
        self.agent_panel_visible.update(cx, |visible, cx| {
            *visible = true;
            cx.notify();
        });
        self.prepare_agent(cx);

        let panel = self.agent_panel.clone();
        if shown {
            panel.update(cx, |panel, cx| panel.focus_composer(window, cx));
        } else {
            // The panel is drawn from the next frame. Next-frame callbacks
            // run before that frame is drawn, so focus one frame later.
            cx.on_next_frame(window, move |_, window, cx| {
                cx.on_next_frame(window, move |_, window, cx| {
                    panel.update(cx, |panel, cx| panel.focus_composer(window, cx));
                });
            });
        }
        cx.notify();
    }

    /// Start the agent where it should work, unless it runs.
    fn prepare_agent(&mut self, cx: &mut Context<Self>) {
        let directory = self.agent_directory(cx);
        self.agent_panel.update(cx, |panel, cx| {
            panel.set_directory(directory, cx);
            panel.shown(cx);
        });
    }

    fn cite_to_agent(&mut self, cite: &Cite, window: &mut Window, cx: &mut Context<Self>) {
        self.show_agent_panel(window, cx);
        self.agent_panel
            .update(cx, |panel, cx| panel.add_cite(cite.clone(), window, cx));
    }

    /// Where the agent works: the Git repository of the collection in the
    /// active tab, so it can read the code its requests call, or the folder
    /// of a collection opened from elsewhere. Otherwise the app's data.
    fn agent_directory(&self, cx: &App) -> PathBuf {
        let collection = self
            .main_view
            .read(cx)
            .active_page()
            .and_then(|page| page.location(cx))
            .map(|location| location.collection.clone());
        let data = self
            .session_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();

        let Some(collection) = collection else {
            return data;
        };
        if let Some(root) = self
            .git
            .read(cx)
            .repositories
            .iter()
            .map(|known| known.repository.root())
            .filter(|root| collection.starts_with(root))
            .max_by_key(|root| root.components().count())
        {
            return root.to_path_buf();
        }
        if collection.starts_with(&data) {
            data
        } else {
            collection
        }
    }

    /// Read saved requests and flows again, which the agent may have
    /// changed. Tabs without unsaved changes follow.
    fn reload_saved(&mut self, cx: &mut Context<Self>) {
        self.collections
            .update(cx, |collections, cx| collections.reload(cx));
        self.flow_panel.update(cx, |flows, cx| flows.refresh(cx));
    }

    fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The new collection is named in the tree, so it must be visible.
        self.reveal_collections(window, cx);

        self.sidebar
            .update(cx, |sidebar, cx| sidebar.create_collection(window, cx));
    }

    fn import_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The imported collection is revealed in the tree, so it must be visible.
        self.reveal_collections(window, cx);

        let environments = self
            .main_view
            .read(cx)
            .environments
            .read(cx)
            .catalog()
            .clone();
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.open_import_dialog(environments, window, cx)
        });
    }

    fn open_collection_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The opened collection is revealed in the tree, so it must be visible.
        self.reveal_collections(window, cx);

        self.sidebar
            .update(cx, |sidebar, cx| sidebar.open_folder(window, cx));
    }

    /// Run a Git action in every repository that holds collections. Without
    /// any, the Git panel opens to start one.
    fn run_git(&mut self, action: GitAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.git.read(cx).repositories.is_empty() {
            self.show_git_panel(true, window, cx);
            return;
        }

        self.git
            .update(cx, |git, cx| git.run(action, None, None, cx));
    }

    fn create_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_git_panel(window, cx);
        self.environments_open = true;
        cx.notify();

        self.main_view
            .update(cx, |view, cx| view.create_environment(window, cx));
    }

    /// Create a flow, open it, and name it in the sidebar.
    fn create_flow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible.update(cx, |visible, cx| {
            *visible = true;
            cx.notify();
        });
        self.leave_git_panel(window, cx);
        self.flows_open = true;
        cx.notify();

        let Some(path) = self.flow_panel.update(cx, |flows, cx| flows.create(cx)) else {
            return;
        };
        if let Some(saved) = self.flow_panel.read(cx).get(&path).cloned() {
            self.open_flow(&saved, window, cx);
        }

        // Opening the tab focuses its canvas, so the name is edited after.
        self.flow_panel
            .update(cx, |flows, cx| flows.begin_rename(path, window, cx));
    }

    fn open_flow(&mut self, saved: &SavedFlow, window: &mut Window, cx: &mut Context<Self>) {
        self.main_view.update(cx, |view, cx| {
            view.open_flow(saved.clone(), cx);
            view.prepare_active_tab(window, cx);
        });
    }

    fn clear_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The confirmation shows in the section, so it must be visible.
        self.leave_git_panel(window, cx);
        self.history_open = true;
        cx.notify();

        self.history
            .update(cx, |history, cx| history.request_clear(window, cx));
    }

    /// How far a section is open, from 0 when folded to 1 when open.
    fn section_progress(&self, section: SidebarSection, window: &mut Window, cx: &mut App) -> f32 {
        let (id, open) = match section {
            SidebarSection::Collections => ("collections-section", self.collections_open),
            SidebarSection::Environments => ("environments-section", self.environments_open),
            SidebarSection::Flows => ("flows-section", self.flows_open),
            SidebarSection::History => ("history-section", self.history_open),
        };

        motion::transition(
            (id, "fold"),
            if open { 1.0 } else { 0.0 },
            Transition::new(Duration::from_millis(200)).ease(ease_in_out_cubic),
            window,
            cx,
        )
    }

    fn section_header(
        &self,
        section: SidebarSection,
        progress: f32,
        count: usize,
        buttons: Vec<Button>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (id, label, open, focus) = match section {
            SidebarSection::Collections => (
                "collections-section",
                "Collections",
                self.collections_open,
                &self.collections_header,
            ),
            SidebarSection::Environments => (
                "environments-section",
                "Environments",
                self.environments_open,
                &self.environments_header,
            ),
            SidebarSection::Flows => (
                "flows-section",
                "Flows",
                self.flows_open,
                &self.flows_header,
            ),
            SidebarSection::History => (
                "history-section",
                "History",
                self.history_open,
                &self.history_header,
            ),
        };
        let focus_visible = focus.is_focused(window) && window.last_input_was_keyboard();

        div().flex_none().h_8().w_full().px_2().child(
            h_flex()
                .size_full()
                .rounded(theme.radius_tokens().md)
                .pr_1()
                .hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
                .child(
                    gpui_kit::base::Button::new(id)
                        .debug_selector(move || id.into())
                        .track_focus(focus)
                        .accessibility_label(label)
                        .aria_expanded(open)
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .justify_start()
                        .pl_2()
                        .gap_2()
                        .rounded(theme.radius_tokens().md)
                        .cursor_default()
                        .when(focus_visible, |this| this.focus_ring_style(window, cx))
                        // Clicking keeps focus where it is, like other buttons.
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_sidebar_section(section, window, cx)
                        }))
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size_3p5()
                                .flex_none()
                                .rotate(percentage(progress / 4.)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label.to_uppercase()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(count.to_string()),
                        ),
                )
                .children(buttons.into_iter().map(|button| button.ghost().xsmall())),
        )
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let frame = v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().sidebar_border);
        if self.git_panel.read(cx).open {
            return frame.child(self.git_panel.clone()).into_any_element();
        }

        let collection_count = self.sidebar.read(cx).collection_count();
        let environment_count = self.main_view.read(cx).environments.read(cx).names().len();
        let flow_count = self.flow_panel.read(cx).count();
        let history_count = self.history.read(cx).count();
        let collections_progress = self.section_progress(SidebarSection::Collections, window, cx);
        let environments_progress = self.section_progress(SidebarSection::Environments, window, cx);
        let flows_progress = self.section_progress(SidebarSection::Flows, window, cx);
        let history_progress = self.section_progress(SidebarSection::History, window, cx);

        // A folding section's rows stay on screen, where a click can still
        // focus them.
        self.release_section_focus(SidebarSection::Collections, window, cx);
        self.release_section_focus(SidebarSection::Environments, window, cx);
        self.release_section_focus(SidebarSection::Flows, window, cx);
        self.release_section_focus(SidebarSection::History, window, cx);

        let new_collection = Button::new("new-collection")
            .debug_selector(|| "new-collection".into())
            .icon(IconName::Plus)
            .tooltip_with_action("New Collection", &NewCollection, Some("Workspace"))
            .on_click(cx.listener(|this, _, window, cx| this.create_collection(window, cx)));
        let open_collection_folder = Button::new("open-collection-folder")
            .debug_selector(|| "open-collection-folder".into())
            .icon(IconName::FolderOpen)
            .tooltip_with_action(
                "Open Folder as Collection",
                &OpenCollectionFolder,
                Some("Workspace"),
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_collection_folder(window, cx)));
        let import_collection = Button::new("import-collection")
            .debug_selector(|| "import-collection".into())
            .icon(Icon::default().path("icons/import.svg"))
            .tooltip_with_action("Import Collection", &ImportCollection, Some("Workspace"))
            .on_click(cx.listener(|this, _, window, cx| this.import_collection(window, cx)));
        let new_environment = Button::new("new-environment")
            .debug_selector(|| "new-environment".into())
            .icon(IconName::Plus)
            .tooltip("New Environment")
            .on_click(cx.listener(|this, _, window, cx| this.create_environment(window, cx)));
        let new_flow = Button::new("new-flow")
            .debug_selector(|| "new-flow".into())
            .icon(IconName::Plus)
            .tooltip_with_action("New Flow", &NewFlow, Some("Workspace"))
            .on_click(cx.listener(|this, _, window, cx| this.create_flow(window, cx)));
        let clear_history = Button::new("clear-history")
            .debug_selector(|| "clear-history".into())
            .icon(Icon::default().path("icons/trash.svg"))
            .tooltip("Clear History")
            .disabled(history_count == 0)
            .on_click(cx.listener(|this, _, window, cx| this.clear_history(window, cx)));

        let border = cx.theme().sidebar_border;
        let divider = move || div().flex_none().mx_2().h(px(1.)).bg(border);

        // Open sections share the height, like the sections of an editor sidebar,
        // and collections, the main one, get twice the share of the others.
        // A folding section gives its share away and clips its rows. An open
        // one does not clip, so focus rings at its edges stay whole.
        let section_body = |progress: f32, share: f32, view: AnyView| {
            div()
                .w_full()
                .flex_basis(relative(0.))
                .flex_grow(progress * share)
                .min_h_0()
                .when(progress < 1.0, |this| this.overflow_hidden())
                .child(view.cached(StyleRefinement::default().size_full()))
        };

        frame
            .pt_1()
            .child(self.section_header(
                SidebarSection::Collections,
                collections_progress,
                collection_count,
                vec![new_collection, open_collection_folder, import_collection],
                window,
                cx,
            ))
            .when(collections_progress > 0.0, |this| {
                this.child(section_body(
                    collections_progress,
                    2.,
                    self.sidebar.clone().into(),
                ))
            })
            .child(divider())
            .child(self.section_header(
                SidebarSection::Environments,
                environments_progress,
                environment_count,
                vec![new_environment],
                window,
                cx,
            ))
            .when(environments_progress > 0.0, |this| {
                this.child(section_body(
                    environments_progress,
                    1.,
                    self.environment_panel.clone().into(),
                ))
            })
            .child(divider())
            .child(self.section_header(
                SidebarSection::Flows,
                flows_progress,
                flow_count,
                vec![new_flow],
                window,
                cx,
            ))
            .when(flows_progress > 0.0, |this| {
                this.child(section_body(
                    flows_progress,
                    1.,
                    self.flow_panel.clone().into(),
                ))
            })
            .child(divider())
            .child(self.section_header(
                SidebarSection::History,
                history_progress,
                history_count,
                vec![clear_history],
                window,
                cx,
            ))
            .when(history_progress > 0.0, |this| {
                this.child(section_body(
                    history_progress,
                    1.,
                    self.history.clone().into(),
                ))
            })
            .into_any_element()
    }

    fn update_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut MainView, &mut Context<MainView>),
    ) {
        self.main_view.update(cx, |view, cx| {
            update(view, cx);
            view.focus(window, cx);
        });
    }
}

/// Tells how a Git action went.
fn git_notification(action: GitAction, errors: &[String]) -> Notification {
    if !errors.is_empty() {
        let title = match action {
            GitAction::Init => "Couldn't start a repository",
            GitAction::Commit => "Couldn't commit",
            GitAction::Pull => "Couldn't pull",
            GitAction::Push => "Couldn't push",
            GitAction::Sync => "Couldn't sync",
            GitAction::Publish => "Couldn't publish",
        };

        return Notification::error(errors.join("\n")).title(title);
    }

    Notification::success(match action {
        GitAction::Init => "Git tracks your collections now.",
        GitAction::Commit => "Committed the collections' changes.",
        GitAction::Pull => "Pulled the latest changes.",
        GitAction::Push => "Pushed your commits.",
        GitAction::Sync => "Your collections are in sync.",
        GitAction::Publish => "Published your collections.",
    })
}

/// Handled outside the workspace's focus, so the status bar's button opens
/// the page wherever focus is.
fn on_open_cookies(workspace: &Entity<Workspace>, window: AnyWindowHandle, cx: &mut App) {
    let workspace = workspace.downgrade();

    cx.on_action(move |_: &OpenCookies, cx| {
        let workspace = workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.close_settings(window, cx);
                    this.update_tabs(window, cx, MainView::open_cookies);
                });
            });
        });
    });
}

fn on_open_settings(workspace: &Entity<Workspace>, window: AnyWindowHandle, cx: &mut App) {
    let workspace = workspace.downgrade();
    let general_workspace = workspace.clone();

    cx.on_action(move |_: &OpenGeneralSettings, cx| {
        let workspace = general_workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.open_settings(window, cx).update(cx, |settings, cx| {
                        settings.select_page(SettingsPage::General, window, cx)
                    });
                });

                window.activate_window();
            });
        });
    });

    let agent_workspace = workspace.clone();
    cx.on_action(move |_: &OpenAgentSettings, cx| {
        let workspace = agent_workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.open_settings(window, cx).update(cx, |settings, cx| {
                        settings.select_page(SettingsPage::Agents, window, cx)
                    });
                });
            });
        });
    });

    cx.on_action(move |_: &OpenSettings, cx| {
        let workspace = workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| this.open_settings(window, cx));

                window.activate_window();
            });
        });
    });
}

pub(crate) fn on_toggle_command_palette(
    workspace: &Entity<Workspace>,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    let workspace = workspace.downgrade();

    cx.on_action(move |_: &ToggleCommandPalette, cx| {
        let workspace = workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| this.toggle_command_palette(window, cx));
            });
        });
    });
}

pub(crate) fn on_toggle_sidebar(
    workspace: &Entity<Workspace>,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    let workspace = workspace.downgrade();
    let git_workspace = workspace.clone();

    cx.on_action(move |_: &ToggleLeftSidebar, cx| {
        let workspace = workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| this.toggle_sidebar(window, cx));
            });
        });
    });

    // The status bar's agent button works wherever focus is.
    let agent_workspace = git_workspace.clone();
    cx.on_action(move |_: &ToggleAgentPanel, cx| {
        let workspace = agent_workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| this.toggle_agent_panel(window, cx));
            });
        });
    });

    // The status bar's Git item switches the sidebar wherever focus is.
    cx.on_action(move |_: &ToggleGitPanel, cx| {
        let workspace = git_workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| this.toggle_git_panel(window, cx));
            });
        });
    });
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep the screen entities alive, but only lay out the visible screen.
        // GPUI still requests child layouts beneath display: none containers.
        if self.settings_visible
            && let Some(settings) = self.settings.clone()
        {
            return div()
                .size_full()
                .text_base()
                .child(settings)
                .into_any_element();
        }

        let sidebar_progress = motion::transition(
            "sidebar-visibility",
            if *self.sidebar_visible.read(cx) {
                1.0
            } else {
                0.0
            },
            Transition::new(Duration::from_millis(150)).ease(ease_in_out_cubic),
            window,
            cx,
        );

        let sidebar_width = self
            .main_split
            .read(cx)
            .sizes()
            .first()
            .copied()
            .unwrap_or(rems(19.).to_pixels(window.rem_size()))
            .clamp(
                rems(14.).to_pixels(window.rem_size()),
                rems(30.).to_pixels(window.rem_size()),
            );

        // Commands that only apply to some tabs stay out of the palette for
        // the others. They are handled here, so they also work while the
        // sidebar has focus.
        let agent_visible = *self.agent_panel_visible.read(cx);
        let active_page = self.main_view.read(cx).active_page();
        let has_url = active_page.is_some_and(Page::is_request);
        let copyable = active_page.is_some_and(Page::can_copy_as_command);

        let workspace = v_flex()
            .size_full()
            .key_context("Workspace")
            .on_action(cx.listener(|this, cite: &CiteToAgent, window, cx| {
                this.cite_to_agent(&cite.0, window, cx);
            }))
            // A tab quotes what has focus in it. Elsewhere there is nothing
            // to cite, so the agent is shown.
            .on_action(cx.listener(|this, _: &AddToAgent, window, cx| {
                this.show_agent_panel(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusSidebarSearch, window, cx| {
                this.reveal_collections(window, cx);

                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.focus_search(window, cx));
            }))
            .on_action(cx.listener(|this, _: &NewCollection, window, cx| {
                this.create_collection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ImportCollection, window, cx| {
                this.import_collection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewFlow, window, cx| {
                this.create_flow(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenCollectionFolder, window, cx| {
                this.open_collection_folder(window, cx);
            }))
            .on_action(cx.listener(|this, _: &GitCommit, window, cx| {
                this.show_git_panel(true, window, cx);
                this.git_panel
                    .update(cx, |panel, cx| panel.begin_commit(window, cx));
            }))
            .on_action(cx.listener(|this, _: &GitPull, window, cx| {
                this.run_git(GitAction::Pull, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GitPush, window, cx| {
                this.run_git(GitAction::Push, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GitSync, window, cx| {
                this.run_git(GitAction::Sync, window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &OpenEnvironmentSelector, window, cx| {
                    this.main_view
                        .update(cx, |view, cx| view.open_environment_picker(window, cx));
                }),
            )
            .on_action(cx.listener(|this, _: &SendRequest, window, cx| {
                this.main_view
                    .update(cx, |view, cx| view.send_request(window, cx));
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| {
                this.main_view
                    .update(cx, |view, cx| view.save_active_request(window, cx));
            }))
            .when(has_url, |this| {
                this.on_action(cx.listener(|this, _: &FocusUrl, window, cx| {
                    this.main_view
                        .update(cx, |view, cx| view.focus_url(window, cx));
                }))
            })
            .when(copyable, |this| {
                this.on_action(cx.listener(|this, _: &CopyAsCurl, window, cx| {
                    this.main_view
                        .update(cx, |view, cx| view.copy_as_command(window, cx));
                }))
            })
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                this.update_tabs(window, cx, MainView::new_tab);
            }))
            .on_action(cx.listener(|this, _: &NewGrpcTab, window, cx| {
                this.update_tabs(window, cx, MainView::new_grpc_tab);
            }))
            .on_action(cx.listener(|this, _: &NewWebSocketTab, window, cx| {
                this.update_tabs(window, cx, MainView::new_websocket_tab);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.update_tabs(window, cx, MainView::close_active_tab);
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.cycle_tab(true, cx));
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.cycle_tab(false, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab1, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(0, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab2, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(1, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab3, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(2, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab4, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(3, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab5, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(4, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab6, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(5, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab7, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(6, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectTab8, window, cx| {
                this.update_tabs(window, cx, |view, cx| view.select_tab(7, cx));
            }))
            .on_action(cx.listener(|this, _: &SelectLastTab, window, cx| {
                this.update_tabs(window, cx, MainView::select_last_tab);
            }))
            // Keep the cached frame aligned with the native title-bar inset.
            .child(
                self.top_panel
                    .clone()
                    .cached(StyleRefinement::default().w_full().h(px(34.)).flex_none()),
            )
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    h_resizable("main_split")
                        .with_state(&self.main_split)
                        .child(
                            resizable_panel()
                                .visible(sidebar_progress > 0.0)
                                .flex_none()
                                .ml(sidebar_width * (sidebar_progress - 1.0))
                                .size(rems(19.).to_pixels(window.rem_size()))
                                .size_range(
                                    rems(14.).to_pixels(window.rem_size())
                                        ..rems(30.).to_pixels(window.rem_size()),
                                )
                                .child(self.sidebar(window, cx)),
                        )
                        .child(self.main_view.clone().into_any_element())
                        .child(
                            resizable_panel()
                                .visible(agent_visible)
                                .flex_none()
                                .size(rems(24.).to_pixels(window.rem_size()))
                                .size_range(
                                    rems(18.).to_pixels(window.rem_size())
                                        ..rems(48.).to_pixels(window.rem_size()),
                                )
                                .child(self.agent_panel.clone()),
                        ),
                ),
            )
            .child(
                self.bottom_panel
                    .clone()
                    .cached(StyleRefinement::default().w_full().h_8().flex_none()),
            );

        div()
            .size_full()
            .text_base()
            .child(workspace)
            .into_any_element()
    }
}

/// Save the session as the window closes, while its bounds can still be
/// read, or when the app quits with the window open.
fn save_session_on_close(workspace: &Entity<Workspace>, window: &Window, cx: &mut App) {
    let closing = workspace.downgrade();
    window.on_window_should_close(cx, move |window, cx| {
        if let Some(workspace) = closing.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.save_session(window, cx));
        }

        true
    });

    let workspace = workspace.downgrade();
    let handle = window.window_handle();
    cx.on_app_quit(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| workspace.save_session(window, cx));
            }
        });

        async {}
    })
    .detach();
}

/// `cookies` is the jar that every request shares, or why it could not be
/// read. `session` is how the workspace last looked; it is saved again
/// shortly after tabs change and when the window closes.
// Each store is loaded once at startup and handed over here.
#[allow(clippy::too_many_arguments)]
pub fn init(
    collections: CollectionRegistry,
    environments: GlobalEnvironments,
    flows: FlowLibrary,
    cookies: Result<request::CookieJar, String>,
    history: History,
    updater: Entity<Updater>,
    session: Session,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    crate::actions::init(cx);
    agent_panel::init(cx);
    tab_ui::Cookies::init(cookies, cx);

    let workspace = cx.new(|cx| {
        Workspace::new(
            collections,
            environments,
            flows,
            history,
            updater,
            session,
            window,
            cx,
        )
    });
    save_session_on_close(&workspace, window, cx);
    on_toggle_sidebar(&workspace, window.window_handle(), cx);
    on_open_settings(&workspace, window.window_handle(), cx);
    on_open_cookies(&workspace, window.window_handle(), cx);
    on_toggle_command_palette(&workspace, window.window_handle(), cx);

    workspace.into()
}
