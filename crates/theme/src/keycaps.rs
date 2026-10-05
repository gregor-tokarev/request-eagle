use gpui_kit::component::{ActiveTheme as _, kbd::Kbd, *};
use gpui_kit::*;

/// A keystroke as a row of keycaps, one for each modifier and one for the key.
pub fn shortcut_keycaps(stroke: &Keystroke, cx: &App) -> AnyElement {
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
                .rounded(cx.theme().radius_tokens().md)
                .bg(cx.theme().foreground.opacity(0.06))
                .text_color(cx.theme().muted_foreground)
                .text_sm()
                .text_center()
                .line_height(rems(1.5))
                .child(label)
        }))
        .into_any_element()
}
