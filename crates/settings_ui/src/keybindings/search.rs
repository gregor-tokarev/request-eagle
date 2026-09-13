use gpui_kit::component::{Size, button::*, input::Input, *};
use gpui_kit::*;
use keybindings_service::Command;

use super::{KeybindingsPage, matches_search, shortcut_keycaps};

impl KeybindingsPage {
    fn toggle_search_recorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_by_shortcut = !self.search_by_shortcut;
        self.search_keystroke = None;
        self.recording = None;
        self.focus_search(window, cx);

        cx.notify();
    }

    pub(super) fn record_search_key(&mut self, stroke: &Keystroke, cx: &mut Context<Self>) {
        if stroke.key.is_empty()
            || matches!(
                stroke.key.as_str(),
                "cmd" | "platform" | "control" | "ctrl" | "alt" | "shift" | "function" | "fn"
            )
        {
            return;
        }

        self.search_keystroke = Some(stroke.clone());

        cx.notify();
    }

    pub(super) fn matches_search(&self, command: &Command, query: &str) -> bool {
        if self.search_by_shortcut {
            self.search_keystroke
                .as_ref()
                .is_none_or(|stroke| matches_shortcut(command, stroke))
        } else {
            matches_search(command, query)
        }
    }

    pub(super) fn render_search(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.search_focus.is_focused(window);

        h_flex()
            .debug_selector(|| "keybindings-search".into())
            .w_full()
            .input_h(Size::Medium)
            .flex_none()
            .gap_2()
            .child(if self.search_by_shortcut {
                h_flex()
                    .id("keybindings-search-recorder")
                    .debug_selector(|| "keybindings-search-recorder".into())
                    .role(Role::Button)
                    .aria_label(
                        "Press a shortcut to search. All keys are captured while focused. \
                         Click outside to stop recording.",
                    )
                    .track_focus(&self.search_focus)
                    .flex_1()
                    .min_w_0()
                    .input_h(Size::Medium)
                    .input_px(Size::Medium)
                    .gap(px(6.))
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(if focused {
                        cx.theme().ring
                    } else {
                        cx.theme().input
                    })
                    .bg(cx.theme().input_background())
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.recording = None;
                            window.focus(&this.search_focus, cx);
                            window.prevent_default();

                            cx.notify();
                        }),
                    )
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        if this.search_focus.is_focused(window) {
                            window.blur(cx);
                            cx.notify();
                        }
                    }))
                    .child(Icon::new(IconName::Search).text_color(cx.theme().muted_foreground))
                    .child(
                        div().flex_1().min_w_0().overflow_hidden().child(
                            match &self.search_keystroke {
                                Some(stroke) => shortcut_keycaps(stroke, cx),
                                None => div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Press shortcut to search…")
                                    .into_any_element(),
                            },
                        ),
                    )
                    .child(
                        Button::new("clear-shortcut-search")
                            .debug_selector(|| "clear-shortcut-search".into())
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("Clear and return to text search")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.clear_search(window, cx)),
                            ),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&self.search)
                            .prefix(IconName::Search)
                            .cleanable(true),
                    )
                    .into_any_element()
            })
            .child(
                Button::new("toggle-shortcut-search")
                    .debug_selector(|| "toggle-shortcut-search".into())
                    .outline()
                    .selected(self.search_by_shortcut)
                    .icon(Icon::default().path("icons/keyboard.svg"))
                    .tooltip(if self.search_by_shortcut {
                        "Search by command name"
                    } else {
                        "Record shortcut to search"
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_search_recorder(window, cx)),
                    ),
            )
    }
}

fn matches_shortcut(command: &Command, stroke: &Keystroke) -> bool {
    command.binding.as_ref().is_some_and(|binding| {
        binding.keystrokes.split_whitespace().any(|keys| {
            Keystroke::parse(keys).is_ok_and(|binding| {
                binding.key == stroke.key && binding.modifiers == stroke.modifiers
            })
        })
    })
}
