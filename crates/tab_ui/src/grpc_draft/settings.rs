use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    switch::Switch,
    *,
};
use gpui_kit::*;
use preferences::Preferences;

use super::draft::GrpcDraft;

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
                .placeholder(self.preferences(cx).max_response_size_mb.to_string())
                .default_value(
                    settings
                        .max_response_message_mb
                        .map(|megabytes| megabytes.to_string())
                        .unwrap_or_default(),
                )
        });

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
        self.server_name = Some(server_name);
        self.max_message = Some(max_message);
    }

    fn preferences(&self, cx: &App) -> request::RequestPreferences {
        cx.try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default()
    }

    pub(super) fn settings_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.settings_inputs(window, cx);

        let settings = &self.request.settings;
        let verify = settings
            .verify_certificates
            .unwrap_or(self.preferences(cx).ssl_certificate_verification);
        let include_defaults = settings.include_default_fields;

        v_flex()
            .debug_selector(|| "grpc-settings".into())
            .w_full()
            .max_w(rems(50.))
            .child(row(
                "Enable server certificate verification",
                "Verify the server certificate when invoking a method over a secure connection. Follows Settings until changed here.",
                div().debug_selector(|| "grpc-verify-certificates".into()).child(
                Switch::new("grpc-verify-certificates")
                    .accessibility_label("Enable server certificate verification")
                    .checked(verify)
                    .on_click(cx.listener(|this, checked, window, cx| {
                        this.request.settings.verify_certificates = Some(*checked);
                        this.schedule_reflection(window, cx);
                        this.redraw(cx);
                    })),
                ),
                cx,
            ))
            .child(row(
                "Override server name for certificate verification",
                "Check the certificate against this name instead of the URL's host.",
                div()
                    .debug_selector(|| "grpc-server-name".into())
                    .w_40()
                    .child(Input::new(self.server_name.as_ref().unwrap()).small()),
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
                            .small()
                            .suffix(div().text_color(cx.theme().muted_foreground).child("MB")),
                    ),
                cx,
            ))
            .into_any_element()
    }
}

/// A setting's title and description beside its control.
fn row(title: &'static str, description: &'static str, control: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .justify_between()
        .gap_4()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(div().flex_none().child(control))
}
