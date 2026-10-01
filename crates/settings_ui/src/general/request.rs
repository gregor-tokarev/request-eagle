use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    switch::Switch,
    *,
};
use gpui_kit::{prelude::*, *};
use preferences::{HttpVersion, Preferences, RequestPreferences};

use crate::layout::{row, section};

const HTTP_VERSIONS: [(HttpVersion, &str); 3] = [
    (HttpVersion::Auto, "Auto"),
    (HttpVersion::Http1_1, "HTTP/1.1"),
    (HttpVersion::Http2, "HTTP/2"),
];

type VersionList = SearchableVec<SharedString>;

pub(super) struct RequestSettings {
    http_version: Entity<SelectState<VersionList>>,
    timeout: Entity<InputState>,
    max_response_size: Entity<InputState>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl RequestSettings {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preferences = cx.global::<Preferences>().request.clone();
        let selected = HTTP_VERSIONS
            .iter()
            .position(|(version, _)| *version == preferences.http_version)
            .unwrap_or(0);

        let http_version = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    HTTP_VERSIONS
                        .iter()
                        .map(|(_, label)| SharedString::from(*label))
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(selected)),
                window,
                cx,
            )
        });
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(preferences.timeout_ms.to_string())
                .validate(|value, _| valid_number(value))
        });
        let max_response_size = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(preferences.max_response_size_mb.to_string())
                .validate(|value, _| valid_number(value))
        });

        let version_subscription = cx.subscribe(
            &http_version,
            |this, _, event: &SelectEvent<VersionList>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some(&(version, _)) = HTTP_VERSIONS
                        .iter()
                        .find(|(_, candidate)| *candidate == label.as_ref())
                {
                    this.save(|request| request.http_version = version, cx);
                }
            },
        );
        let timeout_subscription = cx.subscribe_in(
            &timeout,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Ok(value) = input.read(cx).value().parse::<u64>() {
                        this.save(|request| request.timeout_ms = value, cx);
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let value = cx.global::<Preferences>().request.timeout_ms;
                    input.update(cx, |input, cx| {
                        input.set_value(value.to_string(), window, cx)
                    });
                }
                _ => {}
            },
        );
        let size_subscription = cx.subscribe_in(
            &max_response_size,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Ok(value) = input.read(cx).value().parse::<u64>() {
                        this.save(|request| request.max_response_size_mb = value, cx);
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let value = cx.global::<Preferences>().request.max_response_size_mb;
                    input.update(cx, |input, cx| {
                        input.set_value(value.to_string(), window, cx)
                    });
                }
                _ => {}
            },
        );

        Self {
            http_version,
            timeout,
            max_response_size,
            error: None,
            _subscriptions: vec![
                version_subscription,
                timeout_subscription,
                size_subscription,
            ],
        }
    }

    fn save(&mut self, change: impl FnOnce(&mut RequestPreferences), cx: &mut Context<Self>) {
        self.error = preferences::update(cx, |preferences| change(&mut preferences.request))
            .err()
            .map(|error| format!("Could not save request settings: {error}"));

        cx.notify();
    }
}

fn valid_number(value: &str) -> bool {
    value.is_empty()
        || (value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok())
}

impl Render for RequestSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let verify_ssl = cx
            .global::<Preferences>()
            .request
            .ssl_certificate_verification;
        let follow_all_redirects = cx.global::<Preferences>().request.follow_all_redirects;

        section("Request")
            .child(row(
                "HTTP version",
                "Select the HTTP version to use for sending the request.",
                div().w_40().flex_shrink_0().child(
                    Select::new(&self.http_version)
                        .accessibility_label("HTTP version")
                        .w_full(),
                ),
                cx,
            ))
            .child(row(
                "Request timeout",
                "Set how long a request should wait for a response before timing out. To never time out, set to 0. A request can change this in its Settings tab.",
                div().w_40().flex_shrink_0().child(
                    Input::new(&self.timeout)
                        .suffix(div().text_color(cx.theme().muted_foreground).child("ms"))
                        .w_full(),
                ),
                cx,
            ))
            .child(row(
                "Max response size",
                "Set the maximum size of a response to download. To download a response of any size, set to 0.",
                div().w_40().flex_shrink_0().child(
                    Input::new(&self.max_response_size)
                        .suffix(div().text_color(cx.theme().muted_foreground).child("MB"))
                        .w_full(),
                ),
                cx,
            ))
            .child(row(
                "SSL certificate verification",
                "Verify the server certificate before sending requests. Disabling this allows untrusted certificates. A request can change this in its Settings tab.",
                h_flex().w_40().flex_shrink_0().justify_end().child(
                    Switch::new("ssl-certificate-verification")
                        .accessibility_label("SSL certificate verification")
                        .checked(verify_ssl)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.save(|request| request.ssl_certificate_verification = *checked, cx);
                        })),
                ),
                cx,
            ))
            .child(row(
                "Follow all redirects",
                "Automatically follow HTTP redirects to the final response. A request can change this in its Settings tab.",
                h_flex().w_40().flex_shrink_0().justify_end().child(
                    Switch::new("follow-all-redirects")
                        .accessibility_label("Follow all redirects")
                        .checked(follow_all_redirects)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.save(|request| request.follow_all_redirects = *checked, cx);
                        })),
                ),
                cx,
            ))
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}
