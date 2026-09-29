use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

/// The item count shown beside a request or response section label.
pub(crate) fn section_count(count: usize, selected: bool, cx: &App) -> Div {
    div()
        .flex_none()
        .h_4()
        .min_w_4()
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius_full())
        .bg(if selected {
            cx.theme().background
        } else {
            cx.theme().muted
        })
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(count.to_string())
}
