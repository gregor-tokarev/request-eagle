use gpui_kit::component::{button::*, input::Editor, *};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::definition::DefinitionState;
use super::draft::GrpcDraft;
use crate::variable_input::with_variables;

impl GrpcDraft {
    /// The JSON message editor. Below it, streaming requests get Send and
    /// End Streaming while their call is open.
    pub(super) fn message_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = self.message_state(window, cx);
        let vim = self.message_vim.clone().unwrap();
        let mouse_vim = vim.clone();
        let has_example = matches!(&self.definition, DefinitionState::Loaded(definition)
            if definition.method(self.request.method.trim()).is_some());
        let streams_requests = self
            .call
            .as_ref()
            .map(|call| call.kind.streams_requests())
            .or_else(|| self.method_kind().map(|kind| kind.streams_requests()))
            .unwrap_or(false);
        let sending = self
            .call
            .as_ref()
            .is_some_and(|call| call.kind.streams_requests() && call.is_sending());

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "grpc-message".into())
                    .track_focus(&vim.focus_handle(cx))
                    .capture_any_mouse_down(move |_, _, cx| {
                        mouse_vim.update(cx, |vim, _| vim.mouse_down());
                    })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        with_variables(
                            self.message_completion.as_ref().unwrap(),
                            Editor::new(&message)
                                .h_full()
                                .appearance(false)
                                .bordered(false)
                                .bg(cx
                                    .theme()
                                    .highlight_theme
                                    .style
                                    .editor_background
                                    .unwrap_or_else(|| cx.theme().input_background()))
                                .text_sm()
                                .aria_label("JSON message"),
                        )
                        .h_full(),
                    )
                    .child(crate::vim::cursor(&vim)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .gap_2()
                    .child(
                        Button::new("grpc-example-message")
                            .debug_selector(|| "grpc-example-message".into())
                            .ghost()
                            .small()
                            .icon(Icon::default().path("icons/wand-sparkles.svg"))
                            .label("Use Example Message")
                            .disabled(!has_example)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.use_example_message(window, cx)
                            })),
                    )
                    .child(
                        Button::new("grpc-format-message")
                            .debug_selector(|| "grpc-format-message".into())
                            .ghost()
                            .small()
                            .label("Format")
                            .disabled(!self.message_json_valid)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format_message(window, cx)),
                            ),
                    )
                    .children(self.message_vim.clone())
                    .child(div().flex_1())
                    .when_some(self.send_error.clone(), |row, error| {
                        row.child(
                            div()
                                .debug_selector(|| "grpc-send-error".into())
                                .min_w_0()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .when(streams_requests, |row| {
                        row.child(
                            Button::new("grpc-end-streaming")
                                .debug_selector(|| "grpc-end-streaming".into())
                                .outline()
                                .small()
                                .label("End Streaming")
                                .disabled(!sending)
                                .on_click(cx.listener(|this, _, _, cx| this.end_stream(cx))),
                        )
                        .child(
                            Button::new("grpc-send-message")
                                .debug_selector(|| "grpc-send-message".into())
                                .primary()
                                .small()
                                .label("Send")
                                .disabled(!sending)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.send_message(window, cx)
                                    }),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn format_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.message.clone() else {
            return;
        };
        let formatted = serde_json::from_str::<serde_json::Value>(&self.request.message)
            .and_then(|value| serde_json::to_string_pretty(&value));

        if let Ok(text) = formatted {
            message.update(cx, |message, cx| message.replace_all(text, window, cx));
        }
    }
}
