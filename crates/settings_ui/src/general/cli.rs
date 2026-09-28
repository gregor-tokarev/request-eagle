use gpui_kit::component::{ActiveTheme as _, button::*, h_flex, v_flex};
use gpui_kit::{prelude::*, *};

pub(super) fn install_section(cx: &App) -> impl IntoElement {
    v_flex()
        .debug_selector(|| "cli-install-section".into())
        .w_full()
        .gap_2()
        .child(div().font_weight(FontWeight::MEDIUM).child("Request Eagle CLI"))
        .child(div().text_sm().text_color(cx.theme().muted_foreground)
            .child("Let AI agents manage saved collections, run requests, and edit settings from the terminal. The CLI works independently of the app."))
        .child(div().text_sm().text_color(cx.theme().muted_foreground)
            .child("Optional separate download for macOS and Linux."))
        .child(h_flex().flex_wrap().gap_2()
            .child(Button::new("download-cli").outline().label("Download CLI")
                .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/releases/latest")))
            .child(Button::new("cli-instructions").ghost().label("Installation instructions")
                .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/blob/main/docs/cli.md#install"))))
}
