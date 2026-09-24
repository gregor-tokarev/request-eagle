use gpui_kit::component::{ActiveTheme, GlobalState, tooltip::Tooltip};
use gpui_kit::{prelude::FluentBuilder as _, *};
use keybindings_service::Command;

use super::KeybindingsPage;

// These table controls only need ghost-button styling. Use the base button's
// keyboard, focus and accessibility behavior without the generic button's
// extra content wrapper and unused variant/selected styles on every row.
pub(super) fn row_button(
    id: impl Into<ElementId>,
    label: &'static str,
    disabled: bool,
    cx: &App,
) -> gpui_kit::base::Button {
    let theme = cx.theme();
    let hover = theme
        .accent
        .opacity(if theme.mode.is_dark() { 0.5 } else { 1. });
    let active = theme.tokens.button_active;
    let ring = theme.ring;

    gpui_kit::base::Button::new(id)
        .disabled(disabled)
        .accessibility_label(label)
        .size_6()
        .flex_none()
        .rounded(theme.radius_tokens().sm)
        .cursor_default()
        .text_color(if disabled {
            theme.muted_foreground.opacity(0.5)
        } else {
            theme.secondary_foreground
        })
        .when(!disabled, |this| {
            this.hover(move |style| style.bg(hover))
                .active(move |style| style.bg(active))
                .focus_visible(move |style| {
                    style.shadow(vec![BoxShadow {
                        color: ring.opacity(0.5),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(2.),
                        inset: false,
                    }])
                })
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    GlobalState::suppress_text_selection(cx);
                })
        })
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

pub(super) struct CommandRow {
    pub command: Command,
    pub page: WeakEntity<KeybindingsPage>,
}

impl Render for CommandRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page
            .update(cx, |page, cx| {
                page.render_command(&self.command, crate::geometry::is_narrow(window), cx)
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}
