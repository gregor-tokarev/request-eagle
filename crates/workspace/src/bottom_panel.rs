use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IconNamed, Sizable as _, button::*, h_flex, popover::Popover,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::actions::{OpenCookies, OpenSettings, ToggleGitPanel, ToggleLeftSidebar};
use crate::git_panel::{GitPanel, sync_counts};
use crate::git_state::GitState;

pub(crate) const TOGGLE_SIDEBAR_BUTTON: &str = "toggle-sidebar";

enum SidebarIcon {
    Visible,
    Hidden,
}

impl IconNamed for SidebarIcon {
    fn path(self) -> SharedString {
        // Tabler icons: `layout-sidebar` (filled) and `layout-sidebar-inactive`.
        match self {
            Self::Visible => "icons/layout-sidebar-filled.svg".into(),
            Self::Hidden => "icons/layout-sidebar-inactive.svg".into(),
        }
    }
}

pub struct BottomPanel {
    sidebar_visible: Entity<bool>,
    git: Entity<GitState>,
    git_panel: Entity<GitPanel>,
    _subscriptions: [Subscription; 3],
}

impl BottomPanel {
    pub(crate) fn new(
        sidebar_visible: Entity<bool>,
        git: Entity<GitState>,
        git_panel: Entity<GitPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.observe(&sidebar_visible, |_, _, cx| cx.notify()),
            cx.observe(&git, |_, _, cx| cx.notify()),
            cx.observe(&git_panel, |_, _, cx| cx.notify()),
        ];

        Self {
            sidebar_visible,
            git,
            git_panel,
            _subscriptions: subscriptions,
        }
    }

    /// Where the collections' repositories stand: the branch, the commits to
    /// pull and push, and the changes not committed. It opens the Git panel.
    fn git_status(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let state = self.git.read(cx);
        let repositories = &state.repositories;
        let statuses: Vec<_> = repositories
            .iter()
            .filter_map(|known| known.status())
            .collect();
        let changes: usize = statuses.iter().map(|status| status.changes.len()).sum();
        let ahead: u32 = statuses.iter().map(|status| status.ahead).sum();
        let behind: u32 = statuses.iter().map(|status| status.behind).sum();
        let tracking = statuses.iter().any(|status| status.upstream.is_some());
        let troubled = repositories.iter().any(|known| {
            known.is_interrupted() || known.error.is_some() || matches!(known.status, Some(Err(_)))
        });

        let label = match (state.running, repositories.as_slice()) {
            (Some(action), _) => Some(action.progress().to_owned()),
            (None, []) => None,
            (None, [known]) => Some(
                known
                    .status()
                    .and_then(|status| status.branch.clone())
                    .unwrap_or_else(|| known.repository.name()),
            ),
            (None, repositories) => Some(format!("{} repositories", repositories.len())),
        };

        let trigger = Button::new("git-status")
            .debug_selector(|| "git-status".into())
            .ghost()
            .small()
            .map(|button| {
                if troubled {
                    button.icon(Icon::new(IconName::TriangleAlert).text_color(theme.warning))
                } else {
                    button.icon(Icon::default().path("icons/git-branch.svg"))
                }
            })
            .loading(state.running.is_some())
            .when_some(label, |button, label| button.label(label))
            .accessibility_label("Git")
            .tooltip_with_action("Git", &ToggleGitPanel, Some("Workspace"))
            .when(tracking && (ahead > 0 || behind > 0), |button| {
                button.child(sync_counts(behind, ahead, theme.muted_foreground))
            })
            .when(changes > 0, |button| {
                button.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(div().size(rems(0.375)).rounded_full().bg(theme.warning))
                        .child(changes.to_string()),
                )
            });

        let panel = self.git_panel.clone();
        let open = panel.read(cx).open;
        let focus = panel.read(cx).focus.clone();
        let toggled = panel.downgrade();

        Popover::new("git-panel")
            .anchor(Anchor::BottomLeft)
            .open(open)
            .on_open_change(move |open, _, cx| {
                let _ = toggled.update(cx, |panel, cx| panel.set_open(*open, cx));
            })
            .track_focus(&focus)
            .trigger(trigger)
            .content(move |_, _, _| panel.clone())
    }
}

impl Render for BottomPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = if *self.sidebar_visible.read(cx) {
            SidebarIcon::Visible
        } else {
            SidebarIcon::Hidden
        };

        div()
            .w_full()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .flex()
            .flex_none()
            .items_center()
            .h_8()
            .px_2()
            .bg(cx.theme().tokens.status_bar.background)
            .child(
                div()
                    .flex_none()
                    .debug_selector(|| TOGGLE_SIDEBAR_BUTTON.to_string())
                    .child(
                        Button::new(TOGGLE_SIDEBAR_BUTTON)
                            .ghost()
                            .small()
                            .icon(icon)
                            .accessibility_label("Toggle sidebar")
                            .tooltip_with_action("Toggle sidebar", &ToggleLeftSidebar, None)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(ToggleLeftSidebar.boxed_clone(), cx);
                            }),
                    ),
            )
            .child(div().flex_none().child(self.git_status(cx)))
            .child(div().flex_1())
            .child(
                Button::new("open-cookies")
                    .debug_selector(|| "open-cookies".into())
                    .ghost()
                    .small()
                    .icon(Icon::default().path("icons/cookie.svg"))
                    .accessibility_label("Cookies")
                    .tooltip_with_action("Cookies", &OpenCookies, None)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(OpenCookies.boxed_clone(), cx);
                    }),
            )
            .child(
                Button::new("open-settings")
                    .ghost()
                    .small()
                    .icon(gpui_kit::component::IconName::Settings2)
                    .accessibility_label("Settings")
                    .tooltip_with_action("Settings", &OpenSettings, None)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(OpenSettings.boxed_clone(), cx);
                    }),
            )
    }
}
