use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

// Shared by the ordinary pages and the virtualized appearance/keybinding pages.
pub(crate) const PAGE_WIDTH: Rems = rems(55.);

pub(crate) fn is_narrow(window: &Window) -> bool {
    window.viewport_size().width < rems(42.5).to_pixels(window.rem_size())
}

pub(crate) fn page_inset(window: &Window) -> Rems {
    if is_narrow(window) {
        rems(1.)
    } else {
        rems(2.)
    }
}

/// A titled group of setting rows.
pub(crate) fn section(title: &'static str) -> Div {
    v_flex().w_full().child(
        div()
            .pb_3()
            .text_lg()
            .font_weight(FontWeight::SEMIBOLD)
            .child(title),
    )
}

/// A setting's title and description beside its control. The control wraps
/// below the text when the page is too narrow for both.
pub(crate) fn row(
    title: impl Into<SharedString>,
    description: impl IntoElement,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .justify_between()
        .flex_wrap()
        .gap_4()
        .py_4()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_w(rems(12.5))
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(title.into()))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(control)
}
