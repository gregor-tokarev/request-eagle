use gpui_kit::component::{kbd::Kbd, *};
use gpui_kit::*;

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

pub(super) fn shortcut_keycaps(stroke: &Keystroke, cx: &App) -> AnyElement {
    let mac = cfg!(target_os = "macos");
    let modifiers = [
        (stroke.modifiers.platform, if mac { "⌘" } else { "Win" }),
        (stroke.modifiers.control, if mac { "⌃" } else { "Ctrl" }),
        (stroke.modifiers.alt, if mac { "⌥" } else { "Alt" }),
        (stroke.modifiers.shift, if mac { "⇧" } else { "Shift" }),
        (stroke.modifiers.function, "Fn"),
    ];

    let mut key = stroke.clone();
    key.modifiers = Modifiers::default();

    let labels = modifiers
        .into_iter()
        .filter(|(pressed, _)| *pressed)
        .map(|(_, label)| label.to_owned())
        .chain(std::iter::once(Kbd::format(&key)));

    h_flex()
        .gap_1()
        .children(labels.map(|label| {
            div()
                .h_6()
                .min_w_6()
                .px_1()
                .flex_none()
                .rounded_md()
                .bg(cx.theme().foreground.opacity(0.06))
                .text_color(cx.theme().muted_foreground)
                .text_sm()
                .text_center()
                .line_height(px(24.))
                .child(label)
        }))
        .into_any_element()
}
