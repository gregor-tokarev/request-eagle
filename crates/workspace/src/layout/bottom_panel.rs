use gpui_kit::component::{ActiveTheme as _, IconNamed, Sizable as _, button::*};
use gpui_kit::*;

use crate::actions::{OpenSettings, ToggleLeftSidebar};

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
    _sidebar_visibility_subscription: Subscription,
}

impl BottomPanel {
    pub fn new(sidebar_visible: Entity<bool>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&sidebar_visible, |_, _, cx| cx.notify());

        Self {
            sidebar_visible,
            _sidebar_visibility_subscription: subscription,
        }
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
                            .tooltip_with_action("Toggle Sidebar", &ToggleLeftSidebar, None)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(ToggleLeftSidebar.boxed_clone(), cx);
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("open-settings")
                    .ghost()
                    .small()
                    .icon(gpui_kit::component::IconName::Settings2)
                    .tooltip_with_action("Settings", &OpenSettings, None)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(OpenSettings.boxed_clone(), cx);
                    }),
            )
    }
}
