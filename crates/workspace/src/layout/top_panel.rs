use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

#[derive(Default)]
pub struct TopPanel;

impl Render for TopPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_none()
            .items_center()
            .h(px(34.))
            .pl_20()
            .child("top panel")
    }
}
