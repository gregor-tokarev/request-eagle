use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::{prelude::FluentBuilder as _, *};
use keybindings_service::{self as keybindings, Command};

use super::{KeybindingsPage, shortcut_keycaps};

pub(super) struct Recording {
    pub(super) command: Command,
    pub(super) keystroke: Option<Keystroke>,
    pub(super) error: Option<String>,
}

impl KeybindingsPage {
    pub(super) fn recorder_subscriptions(
        recorder_scope: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Subscription; 2] {
        let view = cx.weak_entity();
        let settings_window = window.window_handle();

        // Intercept before GPUI resolves actions. An ordinary key listener is too
        // late to stop shortcuts such as Quit while the recorder is active.
        let interceptor = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != settings_window {
                return;
            }

            let _ = view.update(cx, |this, cx| {
                // Focus changes immediately on activation; its parent scope is
                // only present after the next paint, so do not wait for that tree.
                if this.search_by_shortcut && this.search_focus.is_focused(window) {
                    this.record_search_key(&event.keystroke, cx);
                } else if this.recording.is_some() && this.recorder_focus.is_focused(window) {
                    this.record_key(&event.keystroke, cx);
                } else {
                    return;
                }

                window.prevent_default();
                cx.stop_propagation();
            });
        });

        let blur_subscription = cx.on_focus_out(recorder_scope, window, |this, _, _, cx| {
            if this.recording.take().is_some() {
                cx.notify();
            }
        });

        [interceptor, blur_subscription]
    }

    pub(super) fn start_recording(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording = Some(Recording {
            command,
            keystroke: None,
            error: None,
        });
        self.error = None;

        window.focus(&self.recorder_focus, cx);

        cx.notify();
    }

    pub(super) fn cancel_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = None;
        self.focus_search(window, cx);

        cx.notify();
    }

    fn record_key(&mut self, stroke: &Keystroke, cx: &mut Context<Self>) {
        if stroke.key.is_empty()
            || matches!(
                stroke.key.as_str(),
                "cmd" | "platform" | "control" | "ctrl" | "alt" | "shift" | "function" | "fn"
            )
        {
            return;
        }

        let Some(recording) = &mut self.recording else {
            return;
        };

        recording.keystroke = Some(stroke.clone());
        recording.error =
            keybindings::validate_override(recording.command.id, &stroke.unparse(), cx)
                .err()
                .map(|e| e.to_string());

        cx.notify();
    }

    fn save_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recording) = &mut self.recording else {
            return;
        };

        if recording.error.is_some() {
            return;
        }

        let Some(stroke) = &recording.keystroke else {
            return;
        };

        match keybindings::set_override(recording.command.id, Some(&stroke.unparse()), cx) {
            Ok(()) => {
                self.error = None;
                self.cancel_recording(window, cx);
            }
            Err(error) => {
                recording.error = Some(error.to_string());

                cx.notify();
            }
        }
    }

    fn remove_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recording) = &mut self.recording else {
            return;
        };

        match keybindings::set_override(recording.command.id, None, cx) {
            Ok(()) => {
                self.error = None;
                self.cancel_recording(window, cx);
            }
            Err(error) => {
                recording.error = Some(error.to_string());

                cx.notify();
            }
        }
    }

    pub(super) fn recorder_buttons(
        &self,
        recording: &Recording,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        h_flex()
            .gap_1()
            .child(
                Button::new("save-recording")
                    .debug_selector(|| "save-recording".into())
                    .ghost()
                    .small()
                    .icon(IconName::Check)
                    .tooltip("Save shortcut")
                    .disabled(recording.keystroke.is_none() || recording.error.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.save_recording(window, cx))),
            )
            .child(
                Button::new("remove-recording")
                    .debug_selector(|| "remove-recording".into())
                    .ghost()
                    .small()
                    .icon(IconName::Delete)
                    .when(recording.command.binding.is_some(), |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .tooltip("Remove shortcut")
                    .disabled(recording.command.binding.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.remove_recording(window, cx))),
            )
            .child(
                Button::new("cancel-recording")
                    .debug_selector(|| "cancel-recording".into())
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Cancel recording")
                    .on_click(cx.listener(|this, _, window, cx| this.cancel_recording(window, cx))),
            )
    }

    pub(super) fn render_recorder(
        &self,
        recording: &Recording,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let accent = if recording.error.is_some() {
            cx.theme().danger
        } else {
            cx.theme().primary
        };

        h_flex()
            .id("inline-keybinding-recorder")
            .debug_selector(|| "inline-keybinding-recorder".into())
            .track_focus(&self.recorder_scope)
            .w_full()
            .h_9()
            .child(
                h_flex()
                    .id("keybinding-recorder-input")
                    .role(Role::Button)
                    .aria_label(format!(
                        "Record shortcut for {}. All keys are captured. Click Save, Cancel, or Remove to finish.",
                        recording.command.label
                    ))
                    .track_focus(&self.recorder_focus)
                    .size_full()
                    .px_3()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(accent)
                    .bg(accent.opacity(0.08))
                    .shadow(vec![BoxShadow {
                        color: accent.opacity(0.18),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(3.),
                        inset: false,
                    }])
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.focus(&this.recorder_focus, cx);
                            window.prevent_default();
                        }),
                    )
                    .child(match &recording.keystroke {
                        Some(stroke) => h_flex()
                            .w_full()
                            .justify_end()
                            .child(shortcut_keycaps(stroke, cx))
                            .into_any_element(),
                        None => h_flex()
                            .gap_1()
                            .child(div().w(px(1.)).h_4().bg(cx.theme().foreground))
                            .child(
                                div()
                                    .text_sm()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Press shortcut"),
                            )
                            .into_any_element(),
                    }),
            )
    }
}
