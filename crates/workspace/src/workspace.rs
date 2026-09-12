use std::{sync::Arc, time::Duration};

use crate::actions::{OpenGeneralSettings, OpenSettings, ToggleLeftSidebar};
use crate::layout::{
    bottom_panel::BottomPanel, main_view::MainView, sidebar::Sidebar, top_panel::TopPanel,
};
use crate::settings::{Settings, SettingsEvent, SettingsPage};
use collection::CollectionRegistry;
use gpui_kit::base::motion::{self, Transition};
use gpui_kit::component::{
    animation::ease_in_out_cubic,
    resizable::{ResizableState, h_resizable, resizable_panel},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use updater::Updater;

pub(super) struct Layout {
    top_panel: Entity<TopPanel>,
    sidebar: Entity<Sidebar>,
    main_view: Entity<MainView>,
    bottom_panel: Entity<BottomPanel>,

    pub(super) main_split: Entity<ResizableState>,
    pub(super) sidebar_visible: Entity<bool>,

    pub(super) settings: Entity<Settings>,
    pub(super) settings_visible: bool,
    previous_focus: Option<FocusHandle>,

    _sidebar_visibility_subscription: Subscription,
    _settings_subscription: Subscription,
    _appearance_subscription: Subscription,
}

impl Layout {
    pub(super) fn new(
        collections: Arc<CollectionRegistry>,
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let appearance_subscription = cx.observe_window_appearance(window, |_, window, cx| {
            request_eagle_theme::apply_preferences(window.appearance(), cx);
        });

        let sidebar_visible = cx.new(|_| true);
        let bottom_panel = cx.new(|cx| BottomPanel::new(sidebar_visible.clone(), cx));
        let sidebar_visibility_subscription = cx.observe(&sidebar_visible, |_, _, cx| cx.notify());

        let settings = cx.new(|cx| Settings::new(updater, window, cx));
        let settings_subscription = cx.subscribe_in(
            &settings,
            window,
            |this, _, _: &SettingsEvent, window, cx| this.close_settings(window, cx),
        );

        Self {
            top_panel: cx.new(|_| TopPanel),
            sidebar: cx.new(|cx| Sidebar::new(collections, window, cx)),
            main_view: cx.new(|_| MainView),
            bottom_panel,
            main_split: cx.new(|_| ResizableState::default()),
            sidebar_visible,
            settings,
            settings_visible: false,
            previous_focus: None,
            _sidebar_visibility_subscription: sidebar_visibility_subscription,
            _settings_subscription: settings_subscription,
            _appearance_subscription: appearance_subscription,
        }
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_visible {
            self.previous_focus = window.focused(cx);
            self.settings_visible = true;
        }

        self.settings
            .update(cx, |settings, cx| settings.focus(window, cx));

        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible.update(cx, |visible, cx| {
            *visible = !*visible;

            cx.notify();
        });
    }
}

fn on_open_settings(layout: &Entity<Layout>, window: AnyWindowHandle, cx: &mut App) {
    let layout = layout.downgrade();
    let general_layout = layout.clone();

    cx.on_action(move |_: &OpenGeneralSettings, cx| {
        let layout = general_layout.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = layout.update(cx, |this, cx| {
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
        let layout = layout.clone();

        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = layout.update(cx, |this, cx| this.open_settings(window, cx));

                window.activate_window();
            });
        });
    });
}

pub(super) fn on_toggle_sidebar(layout: &Entity<Layout>, cx: &mut App) {
    let layout = layout.clone();

    cx.on_action(move |_: &ToggleLeftSidebar, cx| {
        layout.update(cx, |this, cx| this.toggle_sidebar(cx));
    });
}

impl Render for Layout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .unwrap_or(px(300.))
            .clamp(px(220.), px(480.));

        let workspace = v_flex()
            .size_full()
            .when(self.settings_visible, |this| this.hidden())
            .child(self.top_panel.clone())
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    h_resizable("main_split")
                        .with_state(&self.main_split)
                        .child(
                            resizable_panel()
                                .visible(sidebar_progress > 0.0)
                                .flex_none()
                                .ml(sidebar_width * (sidebar_progress - 1.0))
                                .size(px(300.))
                                .size_range(px(220.)..px(480.))
                                .child(self.sidebar.clone()),
                        )
                        .child(self.main_view.clone().into_any_element()),
                ),
            )
            .child(self.bottom_panel.clone());

        div().size_full().text_base().child(workspace).child(
            div()
                .size_full()
                .when(!self.settings_visible, |this| this.hidden())
                .child(self.settings.clone()),
        )
    }
}

pub fn init(collections: CollectionRegistry, updater: Entity<Updater>, cx: &mut App) {
    crate::actions::init(cx);

    let window_options = crate::window_options::use_window_options(cx);
    cx.open_window(window_options, move |window, cx| {
        let layout = cx.new(|cx| Layout::new(Arc::new(collections), updater, window, cx));
        on_toggle_sidebar(&layout, cx);
        on_open_settings(&layout, window.window_handle(), cx);

        cx.new(|cx| Root::new(layout.clone(), window, cx))
    })
    .expect("Failed to open the window");
}
