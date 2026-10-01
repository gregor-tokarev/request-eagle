use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::draft::RequestDraft;
use crate::request_settings::{override_switch, preferences, row, timeout_input, timeout_value};

impl RequestDraft {
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
        let settings = &self.request.settings;
        let preferences = preferences(cx);

        v_flex()
            .debug_selector(|| "request-settings".into())
            .w_full()
            .max_w(rems(50.))
            .child(row(
                "Request timeout",
                "How long to wait for the complete response, in ms. To never time out, set to 0. Empty follows Settings.",
                div()
                    .debug_selector(|| "request-timeout".into())
                    .w_40()
                    .child(
                        Input::new(&timeout)
                            .suffix(div().text_color(cx.theme().muted_foreground).child("ms")),
                    ),
                cx,
            ))
            .child(row(
                "Follow redirects",
                "Automatically follow HTTP redirects to the final response. Follows Settings until changed here.",
                override_switch(
                    "request-follow-redirects",
                    "Follow redirects",
                    settings.follow_redirects,
                    preferences.follow_all_redirects,
                    cx.listener(|this, value: &Option<bool>, _, cx| {
                        this.request.settings.follow_redirects = *value;
                        cx.notify();
                    }),
                ),
                cx,
            ))
            .child(row(
                "Enable server certificate verification",
                "Verify the server certificate before sending the request. Follows Settings until changed here.",
                override_switch(
                    "request-verify-certificates",
                    "Enable server certificate verification",
                    settings.verify_certificates,
                    preferences.ssl_certificate_verification,
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
