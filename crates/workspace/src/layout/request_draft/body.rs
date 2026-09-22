use collection::Method;
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, InputEvent},
    *,
};
use gpui_kit::*;

use super::draft::RequestDraft;

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
        self._subscriptions
            .push(cx.subscribe(&body, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    this.request.body = (!value.is_empty()).then(|| value.as_bytes().to_vec());
                    this.refresh_generated_headers(cx);
                    cx.notify();
                }
            }));
        self.body = Some(body.clone());

        body
    }

    pub(super) fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let modes = self.body_modes(cx);
        let content = if self.request.form.is_some() {
            self.form_state(window, cx).into_any_element()
        } else {
            self.raw_body(window, cx)
        };

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(modes)
            .child(content)
            .into_any_element()
    }

    fn raw_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self.body_state(window, cx);

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(28.))
                    .gap_2()
                    .child(div().text_color(cx.theme().muted_foreground).child("Raw"))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().info)
                            .child("JSON"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("format-request-json")
                            .debug_selector(|| "format-request-json".into())
                            .ghost()
                            .small()
                            .label("Format")
                            .disabled(self.request.body.as_ref().is_none_or(|body| {
                                serde_json::from_slice::<serde_json::Value>(body).is_err()
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(value) = this.request.body.as_ref().and_then(|body| {
                                    serde_json::from_slice::<serde_json::Value>(body).ok()
                                }) {
                                    let text = serde_json::to_string_pretty(&value).unwrap();
                                    if let Some(body) = &this.body {
                                        body.update(cx, |body, cx| {
                                            body.replace_all(text, window, cx)
                                        });
                                    }
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "request-body".into())
                    .flex_1()
                    .min_h_0()
                    .child(
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
                            .text_size(px(13.))
                            .aria_label("JSON request body"),
                    ),
            )
            .into_any_element()
    }
}
