use gpui_kit::component::{ActiveTheme as _, TitleBar};
use gpui_kit::*;

pub struct TopPanel;

impl Render for TopPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // macOS draws the window controls over this bar. Elsewhere the title
        // bar draws them and moves the window, unless the system decorates it.
        if !cfg!(target_os = "macos") {
            return TitleBar::new()
                .bg(cx.theme().tokens.title_bar.background)
                .text_sm()
                .child("Request Eagle")
                .into_any_element();
        }

        div()
            .w_full()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().tokens.title_bar.background)
            .flex()
            .flex_none()
            .items_center()
            // Match native title-bar chrome; it does not scale with content zoom.
            .h(px(34.))
            .pl_20()
            .text_sm()
            .child("Request Eagle")
            .into_any_element()
    }
}
