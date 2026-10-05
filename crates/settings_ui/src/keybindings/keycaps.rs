use gpui_kit::component::*;
use gpui_kit::*;
use request_eagle_theme::shortcut_keycaps;

pub(super) fn shortcut(keys: Option<&str>, cx: &App) -> AnyElement {
    match keys {
        Some(keys) => h_flex()
            .w_full()
            .justify_end()
            .gap_1()
            .children(
                keys.split_whitespace()
                    .filter_map(|key| Keystroke::parse(key).ok())
                    .map(|stroke| shortcut_keycaps(&stroke, cx)),
            )
            .into_any_element(),
        None => div()
            .w_full()
            .text_right()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Not set")
            .into_any_element(),
    }
}
