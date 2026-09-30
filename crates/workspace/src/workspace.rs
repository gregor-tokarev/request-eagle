use std::time::Duration;

use crate::actions::*;
use crate::{
    bottom_panel::BottomPanel,
    command_palette::CommandPalette,
    environment_panel::{EnvironmentPanel, EnvironmentPanelEvent},
    main_view::MainView,
    top_panel::TopPanel,
};
use collection::CollectionRegistry;
use collections_panel_ui::{CollectionPanel, CollectionPanelEvent};
use environment::GlobalEnvironments;
use gpui_kit::base::motion::{self, Transition};
use gpui_kit::component::{
    animation::ease_in_out_cubic,
    button::{Button, ButtonVariants},
    resizable::{ResizableState, h_resizable, resizable_panel},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use settings_ui::{Settings, SettingsEvent, SettingsPage};
use tab_ui::{Environments, RequestLocation};
use updater::Updater;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarSection {
    Collections,
    Environments,
}

pub(crate) struct Workspace {
    top_panel: Entity<TopPanel>,
    pub(crate) sidebar: Entity<CollectionPanel>,
    pub(crate) environment_panel: Entity<EnvironmentPanel>,
    collections_open: bool,
    environments_open: bool,
    pub(crate) collections_header: FocusHandle,
    environments_header: FocusHandle,
    pub(crate) main_view: Entity<MainView>,
    bottom_panel: Entity<BottomPanel>,

    pub(crate) main_split: Entity<ResizableState>,
    pub(crate) sidebar_visible: Entity<bool>,

    pub(crate) settings: Entity<Settings>,
    pub(crate) settings_visible: bool,
    previous_focus: Option<FocusHandle>,

    pub(crate) command_palette: Option<WeakEntity<list::ListState<CommandPalette>>>,

    _sidebar_subscription: Subscription,
    _environment_panel_subscription: Subscription,
    _settings_subscription: Subscription,
}

impl Workspace {
    pub(crate) fn new(
        collections: CollectionRegistry,
        environments: GlobalEnvironments,
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sidebar_visible = cx.new(|_| true);
        // The bottom panel is cached, so it observes the visibility itself.
        let bottom_panel = cx.new(|cx| BottomPanel::new(sidebar_visible.clone(), cx));

        let settings = cx.new(|cx| Settings::new(updater, window, cx));
        let settings_subscription = cx.subscribe_in(
            &settings,
            window,
            |this, _, _: &SettingsEvent, window, cx| this.close_settings(window, cx),
        );

        let sidebar = cx.new(|cx| CollectionPanel::new(collections, window, cx));
        let sidebar_subscription =
            cx.subscribe_in(&sidebar, window, |this, _, event, window, cx| match event {
                CollectionPanelEvent::OpenCollection {
                    path,
                    name,
                    variables,
                    scripts,
                } => {
                    this.main_view.update(cx, |view, cx| {
                        view.open_collection(
                            path,
                            name.clone(),
                            variables.clone(),
                            scripts.clone(),
                            window,
                            cx,
                        );
                        view.prepare_active_tab(window, cx);
                    });
                }
                CollectionPanelEvent::CollectionDeleted { path } => {
                    this.main_view
                        .update(cx, |view, cx| view.close_collection(path, cx));
                }
                CollectionPanelEvent::CollectionRenamed {
                    previous_path,
                    path,
                    name,
                } => {
                    this.main_view.update(cx, |view, cx| {
                        view.relocate_collection(previous_path, path, name.clone(), window, cx);
                    });
                }
                CollectionPanelEvent::RequestRelocated {
                    id,
                    previous_path,
                    path,
                    name,
                    collection,
                    folders,
                } => {
                    let location = RequestLocation {
                        path: path.clone(),
                        id: id.clone(),
                        name: name.clone(),
                        collection: collection.clone(),
                        folders: folders.clone(),
                    };

                    this.main_view.update(cx, |view, cx| {
                        view.relocate_request(previous_path, location, cx);
                    });
                }
                CollectionPanelEvent::OpenRequest {
                    id,
                    path,
                    name,
                    collection,
                    folders,
                    request,
                } => {
                    let location = RequestLocation {
                        path: path.clone(),
                        id: id.clone(),
                        name: name.clone(),
                        collection: collection.clone(),
                        folders: folders.clone(),
                    };

                    this.main_view.update(cx, |view, cx| {
                        view.open_request(location, request, cx);
                        view.prepare_active_tab(window, cx);
                    });
                }
            });
        window.focus(&sidebar.focus_handle(cx), cx);

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

        let main_view = cx.new(|cx| MainView::new(environments, sidebar.clone(), window, cx));
        main_view.update(cx, |view, cx| view.prepare_active_tab(window, cx));

        Self {
            top_panel: cx.new(|_| TopPanel),
            sidebar,
            environment_panel,
            collections_open: true,
            environments_open: true,
            collections_header: cx.focus_handle(),
            environments_header: cx.focus_handle(),
            main_view,
            bottom_panel,
            main_split: cx.new(|_| ResizableState::default()),
            sidebar_visible,
            settings,
            settings_visible: false,
            previous_focus: None,
            command_palette: None,
            _sidebar_subscription: sidebar_subscription,
            _environment_panel_subscription: environment_panel_subscription,
            _settings_subscription: settings_subscription,
        }
    }

    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_visible {
            self.previous_focus = window.focused(cx);
            self.settings_visible = true;
        }

        self.settings
            .update(cx, |settings, cx| settings.focus(window, cx));

        cx.notify();
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

    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible.update(cx, |visible, cx| {
            *visible = !*visible;

            cx.notify();
        });

        cx.notify();
    }

    pub(crate) fn toggle_sidebar_section(
        &mut self,
        section: SidebarSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (open, contains_focus, header) = match section {
            SidebarSection::Collections => {
                self.collections_open = !self.collections_open;

                (
                    self.collections_open,
                    self.sidebar.read(cx).contains_focus(window, cx),
                    &self.collections_header,
                )
            }
            SidebarSection::Environments => {
                self.environments_open = !self.environments_open;

                (
                    self.environments_open,
                    self.environment_panel
                        .focus_handle(cx)
                        .contains_focused(window, cx),
                    &self.environments_header,
                )
            }
        };

        // Keys must not go to the rows of a folded section. Its header can
        // open it again.
        if !open && contains_focus {
            window.focus(header, cx);
        }

        cx.notify();
    }

    fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The new collection is named in the tree, so it must be visible.
        self.collections_open = true;
        cx.notify();

        self.sidebar
            .update(cx, |sidebar, cx| sidebar.create_collection(window, cx));
    }

    fn create_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.environments_open = true;
        cx.notify();

        self.main_view
            .update(cx, |view, cx| view.create_environment(window, cx));
    }

    fn section_header(
        &self,
        section: SidebarSection,
        count: usize,
        new_button: Button,
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
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size_3p5()
                            .flex_none(),
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
                .child(new_button.ghost().xsmall()),
        )
    }

    fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let collection_count = self.sidebar.read(cx).collection_count();
        let environment_count = self.main_view.read(cx).environments.read(cx).names().len();

        // Open sections share the height, like the sections of an editor sidebar.
        let section_body = StyleRefinement::default().w_full().flex_1().min_h_0();

        v_flex()
            .size_full()
            .pt_1()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                self.section_header(
                    SidebarSection::Collections,
                    collection_count,
                    Button::new("new-collection")
                        .debug_selector(|| "new-collection".into())
                        .icon(IconName::Plus)
                        .tooltip("New Collection")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.create_collection(window, cx)),
                        ),
                    window,
                    cx,
                ),
            )
            .when(self.collections_open, |this| {
                this.child(self.sidebar.clone().cached(section_body.clone()))
            })
            .child(
                div()
                    .flex_none()
                    .mx_2()
                    .h(px(1.))
                    .bg(cx.theme().sidebar_border),
            )
            .child(
                self.section_header(
                    SidebarSection::Environments,
                    environment_count,
                    Button::new("new-environment")
                        .debug_selector(|| "new-environment".into())
                        .icon(IconName::Plus)
                        .tooltip("New Environment")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.create_environment(window, cx)),
                        ),
                    window,
                    cx,
                ),
            )
            .when(self.environments_open, |this| {
                this.child(self.environment_panel.clone().cached(section_body))
            })
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

fn on_open_settings(workspace: &Entity<Workspace>, window: AnyWindowHandle, cx: &mut App) {
    let workspace = workspace.downgrade();
    let general_workspace = workspace.clone();

    cx.on_action(move |_: &OpenGeneralSettings, cx| {
        let workspace = general_workspace.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = workspace.update(cx, |this, cx| {
                    this.open_settings(window, cx);

                    this.settings.update(cx, |settings, cx| {
                        settings.select_page(SettingsPage::General, window, cx)
                    });
                });

                window.activate_window();
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

pub(crate) fn on_toggle_sidebar(workspace: &Entity<Workspace>, cx: &mut App) {
    let workspace = workspace.clone();

    cx.on_action(move |_: &ToggleLeftSidebar, cx| {
        workspace.update(cx, |this, cx| this.toggle_sidebar(cx));
    });
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep the screen entities alive, but only lay out the visible screen.
        // GPUI still requests child layouts beneath display: none containers.
        if self.settings_visible {
            return div()
                .size_full()
                .text_base()
                .child(self.settings.clone())
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

        let workspace = v_flex()
            .size_full()
            .key_context("Workspace")
            .on_action(cx.listener(|this, _: &FocusSidebarSearch, window, cx| {
                this.sidebar_visible.update(cx, |visible, cx| {
                    *visible = true;
                    cx.notify();
                });
                this.collections_open = true;
                cx.notify();

                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.focus_search(window, cx));
            }))
            .on_action(cx.listener(|this, _: &SendRequest, window, cx| {
                this.main_view
                    .update(cx, |view, cx| view.send_request(window, cx));
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| {
                this.main_view
                    .update(cx, |view, cx| view.save_active_request(window, cx));
            }))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                this.update_tabs(window, cx, MainView::new_tab);
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
                        .child(self.main_view.clone().into_any_element()),
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
            .children(Root::render_dialog_layer(window, cx))
            .into_any_element()
    }
}

pub fn init(
    collections: CollectionRegistry,
    environments: GlobalEnvironments,
    updater: Entity<Updater>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    crate::actions::init(cx);

    let workspace = cx.new(|cx| Workspace::new(collections, environments, updater, window, cx));
    on_toggle_sidebar(&workspace, cx);
    on_open_settings(&workspace, window.window_handle(), cx);
    on_toggle_command_palette(&workspace, window.window_handle(), cx);

    workspace.into()
}
