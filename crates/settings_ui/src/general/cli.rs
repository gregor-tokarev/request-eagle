use gpui_kit::component::{ActiveTheme as _, IconName, button::*, h_flex, v_flex};
use gpui_kit::{prelude::*, *};

pub(super) fn install_section(cx: &App) -> impl IntoElement {
    v_flex()
        .debug_selector(|| "cli-install-section".into())
        .w_full()
        .child(div().pb_3().text_lg().font_weight(FontWeight::SEMIBOLD).child("Agent CLI"))
        .child(
            v_flex()
                .w_full()
                .gap_1()
                .py_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().font_weight(FontWeight::MEDIUM).child("Request Eagle CLI"))
                .child(div().text_sm().text_color(cx.theme().muted_foreground)
                    .child("Let AI agents manage saved collections, run requests, and edit settings from the terminal. The CLI works independently of the app and is a separate download for macOS and Linux."))
                .child(h_flex().pt_2().flex_wrap().gap_2()
                    .child(Button::new("download-cli").outline().label("Download CLI")
                        .icon(IconName::ExternalLink)
                        .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/releases/latest")))
                    .child(Button::new("cli-instructions").ghost().label("Installation instructions")
                        .icon(IconName::ExternalLink)
                        .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/blob/main/docs/cli.md#install")))),
        )
}
