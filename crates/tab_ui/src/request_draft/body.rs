use collection::Method;
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, InputEvent},
    *,
};
use gpui_kit::*;

use super::draft::RequestDraft;
use crate::variable_input::{VariableInput, VariableTarget, with_variables};

impl RequestDraft {
    pub(super) fn supports_body(&self) -> bool {
        !matches!(self.request.method, Method::Get | Method::Head)
    }

    pub(super) fn body_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        if let Some(body) = &self.body {
            return body.clone();
        }

        let value = self
            .request
            .body
            .as_deref()
            .map(String::from_utf8_lossy)
            .unwrap_or_default()
            .into_owned();
        let body = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .soft_wrap(true)
                .placeholder("Enter JSON request body")
                .default_value(value)
        });
        let scope = self.variables(cx);
        self.body_vim = Some(cx.new(|cx| crate::vim::Vim::new(body.clone(), cx)));
        self.body_completion =
            Some(cx.new(|cx| {
                VariableInput::new(VariableTarget::Editor(body.clone()), scope, window, cx)
            }));
        self._subscriptions
            .push(cx.subscribe(&body, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    this.request.body = (!value.is_empty()).then(|| value.as_bytes().to_vec());
                    this.validate_body(value, cx);
                    this.refresh_generated_headers(cx);
                    cx.notify();
                }
            }));
        self.body = Some(body.clone());
        self.validate_body(body.read(cx).value(), cx);

        body
    }

    fn validate_body(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.body_json_valid = false;

        // A new edit drops the previous validation/formatting task, so an old
        // result cannot enable Format or overwrite a more recent body.
        let task = cx
            .background_executor()
            .spawn(async move { serde_json::from_str::<serde_json::Value>(&text).is_ok() });
        self.body_task = Some(cx.spawn(async move |this, cx| {
            let valid = task.await;
            let _ = this.update(cx, |this, cx| {
                this.body_json_valid = valid;
                this.body_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn format_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.body_json_valid || self.body_task.is_some() {
            return;
        }

        let Some(body) = &self.body else { return };
        let text = body.read(cx).value();
        let source = text.clone();
        let task = cx.background_executor().spawn(async move {
            serde_json::from_str::<serde_json::Value>(&text)
                .and_then(|value| serde_json::to_string_pretty(&value))
        });
        self.body_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.body_task = None;

                if let Ok(text) = result
                    && let Some(body) = &this.body
                    && body.read(cx).value() == source
                {
                    body.update(cx, |body, cx| body.replace_all(text, window, cx));
                }

                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self.body_state(window, cx);
        let mouse_vim = self.body_vim.as_ref().unwrap().clone();

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .flex_none()
                    .h_7()
                    .gap_2()
                    .child(div().text_color(cx.theme().muted_foreground).child("Raw"))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(cx.theme().radius_tokens().md)
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().info)
                            .child("JSON"),
                    )
                    .child(div().flex_1())
                    .children(self.body_vim.clone())
                    .child(
                        Button::new("format-request-json")
                            .debug_selector(|| "format-request-json".into())
                            .ghost()
                            .small()
                            .label("Format")
                            .disabled(!self.body_json_valid || self.body_task.is_some())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format_body(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "request-body".into())
                    .track_focus(&self.body_vim.as_ref().unwrap().focus_handle(cx))
                    .capture_any_mouse_down(move |_, _, cx| {
                        mouse_vim.update(cx, |vim, _| vim.mouse_down());
                    })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        with_variables(
                            self.body_completion.as_ref().unwrap(),
                            Editor::new(&body)
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
                                .aria_label("JSON request body"),
                        )
                        .h_full(),
                    )
                    .child(crate::vim::cursor(self.body_vim.as_ref().unwrap())),
            )
            .into_any_element()
    }
}
