use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::draft::WebSocketDraft;
use crate::request_settings::{override_switch, preferences, row, timeout_input, timeout_value};

impl WebSocketDraft {
    pub(super) fn timeout_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(timeout) = &self.timeout {
            return timeout.clone();
        }

        let timeout = cx.new(|cx| timeout_input(self.request.settings.timeout_ms, window, cx));
        self._subscriptions.push(
            cx.subscribe(&timeout, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.request.settings.timeout_ms = timeout_value(input.read(cx));
                    cx.notify();
                }
            }),
        );
        self.timeout = Some(timeout.clone());

        timeout
    }

    pub(super) fn settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let timeout = self.timeout_state(window, cx);

        v_flex()
            .debug_selector(|| "websocket-settings".into())
            .w_full()
            .max_w(rems(50.))
            .child(row(
                "Connection timeout",
                "How long to wait for the server to accept the connection, in ms. To never time out, set to 0. Empty follows Settings.",
                div()
                    .debug_selector(|| "websocket-timeout".into())
                    .w_40()
                    .child(
                        Input::new(&timeout)
                            .suffix(div().text_color(cx.theme().muted_foreground).child("ms")),
                    ),
                cx,
            ))
            .child(row(
                "Enable server certificate verification",
                "Verify the server certificate when connecting over a secure connection. Follows Settings until changed here.",
                override_switch(
                    "websocket-verify-certificates",
                    "Enable server certificate verification",
                    self.request.settings.verify_certificates,
                    preferences(cx).ssl_certificate_verification,
                    cx.listener(|this, value: &Option<bool>, _, cx| {
                        this.request.settings.verify_certificates = *value;
                        cx.notify();
                    }),
                ),
                cx,
            ))
            .into_any_element()
    }
}
