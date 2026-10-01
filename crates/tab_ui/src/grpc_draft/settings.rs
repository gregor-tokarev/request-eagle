use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    switch::Switch,
    *,
};
use gpui_kit::*;

use super::draft::GrpcDraft;
use crate::request_settings::{override_switch, preferences, row, timeout_input, timeout_value};

impl GrpcDraft {
    /// Create the settings inputs when the Settings tab is first shown.
    pub(super) fn settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_name.is_some() {
            return;
        }

        let settings = &self.request.settings;
        let server_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Host name")
                .default_value(settings.server_name.clone())
        });
        let max_message = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(preferences(cx).max_response_size_mb.to_string())
                .default_value(
                    settings
                        .max_response_message_mb
                        .map(|megabytes| megabytes.to_string())
                        .unwrap_or_default(),
                )
        });
        let timeout = cx.new(|cx| timeout_input(settings.timeout_ms, window, cx));

        self._subscriptions.push(cx.subscribe_in(
            &server_name,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.request.settings.server_name = input.read(cx).value().trim().to_owned();
                    this.schedule_reflection(window, cx);
                    cx.notify();
                }
            },
        ));
        self._subscriptions.push(cx.subscribe(
            &max_message,
            |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();

                    // An empty or invalid value follows the preference.
                    this.request.settings.max_response_message_mb = value.trim().parse().ok();
                    cx.notify();
                }
            },
        ));
        self._subscriptions.push(
            cx.subscribe(&timeout, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.request.settings.timeout_ms = timeout_value(input.read(cx));
                    cx.notify();
                }
            }),
        );
        self.server_name = Some(server_name);
        self.max_message = Some(max_message);
        self.timeout = Some(timeout);
    }

    pub(super) fn settings_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.settings_inputs(window, cx);

        let settings = &self.request.settings;
        let include_defaults = settings.include_default_fields;

        v_flex()
            .debug_selector(|| "grpc-settings".into())
            .w_full()
            .max_w(rems(50.))
            .child(row(
                "Enable server certificate verification",
                "Verify the server certificate when invoking a method over a secure connection. Follows Settings until changed here.",
                override_switch(
                    "grpc-verify-certificates",
                    "Enable server certificate verification",
                    settings.verify_certificates,
                    preferences(cx).ssl_certificate_verification,
                    cx.listener(|this, value: &Option<bool>, window, cx| {
                        this.request.settings.verify_certificates = *value;
                        this.schedule_reflection(window, cx);
                        this.redraw(cx);
                    }),
                ),
                cx,
            ))
            .child(row(
                "Override server name for certificate verification",
                "Check the certificate against this name instead of the URL's host.",
                div()
                    .debug_selector(|| "grpc-server-name".into())
                    .w_40()
                    .child(Input::new(self.server_name.as_ref().unwrap())),
                cx,
            ))
            .child(row(
                "Request timeout",
                "How long to wait for a unary call or server reflection, in ms. Streams stay open until they end. To never time out, set to 0. Empty follows Settings.",
                div()
                    .debug_selector(|| "grpc-timeout".into())
                    .w_40()
                    .child(
                        Input::new(self.timeout.as_ref().unwrap())
                            .suffix(div().text_color(cx.theme().muted_foreground).child("ms")),
                    ),
                cx,
            ))
            .child(row(
                "Include fields with default values in the response",
                "Show response fields with default values, such as empty strings and zeros. Turn this off to leave them out.",
                div().debug_selector(|| "grpc-include-default-fields".into()).child(
                Switch::new("grpc-include-default-fields")
                    .accessibility_label("Include fields with default values in the response")
                    .checked(include_defaults)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.request.settings.include_default_fields = *checked;
                        this.redraw(cx);
                    })),
                ),
                cx,
            ))
            .child(row(
                "Maximum response message size",
                "The largest message to receive, in MB. To receive messages of any size, set to 0. Empty follows Settings.",
                div()
                    .debug_selector(|| "grpc-max-message".into())
                    .w_40()
                    .child(
                        Input::new(self.max_message.as_ref().unwrap())
                            .suffix(div().text_color(cx.theme().muted_foreground).child("MB")),
                    ),
                cx,
            ))
            .into_any_element()
    }
}
