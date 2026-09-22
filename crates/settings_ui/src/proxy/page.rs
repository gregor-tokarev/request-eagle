use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    switch::Switch,
    *,
};
use gpui_kit::{prelude::*, *};
use preferences::{Preferences, ProxyMode, ProxyPreferences, ProxyProtocol};

type Options = SearchableVec<SharedString>;

const MODES: [(ProxyMode, &str); 3] = [
    (ProxyMode::System, "Use system proxy"),
    (ProxyMode::Custom, "Use custom proxy"),
    (ProxyMode::Disabled, "No proxy"),
];

pub(crate) struct ProxySettings {
    pub(super) draft: ProxyPreferences,
    pub(super) mode: Entity<SelectState<Options>>,
    pub(super) protocol: Entity<SelectState<Options>>,
    pub(super) host: Entity<InputState>,
    pub(super) port: Entity<InputState>,
    pub(super) username: Entity<InputState>,
    pub(super) password: Entity<InputState>,
    bypass: Entity<InputState>,
    pub(super) error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ProxySettings {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        preferences::init(cx);

        let draft = cx.global::<Preferences>().request.proxy.clone();
        let mode = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    MODES
                        .iter()
                        .map(|(_, label)| (*label).into())
                        .collect::<Vec<_>>(),
                ),
                MODES
                    .iter()
                    .position(|(mode, _)| *mode == draft.mode)
                    .map(IndexPath::new),
                window,
                cx,
            )
        });
        let protocol = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec!["http".into(), "https".into()]),
                Some(IndexPath::new(usize::from(
                    draft.protocol == ProxyProtocol::Https,
                ))),
                window,
                cx,
            )
        });
        let host = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Hostname or full proxy URL")
                .default_value(draft.host.clone())
        });
        let port = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("8080")
                .default_value(draft.port.to_string())
        });
        let username =
            cx.new(|cx| InputState::new(window, cx).default_value(draft.username.clone()));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(draft.password.clone())
                .masked(true)
        });
        let bypass = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("127.0.0.1, localhost, *.example.com")
                .default_value(draft.bypass.clone())
        });
        let mut subscriptions = vec![
            cx.subscribe(&mode, |this, _, event: &SelectEvent<Options>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some((mode, _)) =
                        MODES.iter().find(|(_, value)| *value == label.as_ref())
                {
                    this.draft.mode = *mode;
                    this.save(cx);
                }
            }),
            cx.subscribe(&protocol, |this, _, event: &SelectEvent<Options>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    this.draft.protocol = if label == "https" {
                        ProxyProtocol::Https
                    } else {
                        ProxyProtocol::Http
                    };
                    this.save(cx);
                }
            }),
        ];

        for input in [&host, &port, &username, &password, &bypass] {
            subscriptions.push(cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save(cx);
                }
            }));
        }

        Self {
            draft,
            mode,
            protocol,
            host,
            port,
            username,
            password,
            bypass,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn paste_proxy_url(
        &mut self,
        clipboard: &ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(value) = clipboard.text().filter(|value| value.contains("://")) else {
            return false;
        };
        let proxy = match ProxyPreferences::from_url(&value) {
            Ok(proxy) => proxy,
            Err(error) => {
                self.error = Some(error.into());
                cx.notify();

                return true;
            }
        };

        self.draft.protocol = proxy.protocol;
        self.draft.authentication = proxy.authentication;
        self.protocol.update(cx, |select, cx| {
            select.set_selected_index(
                Some(IndexPath::new(usize::from(
                    proxy.protocol == ProxyProtocol::Https,
                ))),
                window,
                cx,
            );
        });
        self.host
            .update(cx, |input, cx| input.set_value(proxy.host, window, cx));
        self.port.update(cx, |input, cx| {
            input.set_value(proxy.port.to_string(), window, cx)
        });
        self.username
            .update(cx, |input, cx| input.set_value(proxy.username, window, cx));
        self.password.update(cx, |input, cx| {
            input.set_masked(true, window, cx);
            input.set_value(proxy.password, window, cx);
        });
        self.save(cx);

        true
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let mut proxy = self.draft.clone();
        proxy.host = self.host.read(cx).value().trim().to_owned();
        proxy.username = self.username.read(cx).value().to_string();
        proxy.password = self.password.read(cx).value().to_string();
        proxy.bypass = self.bypass.read(cx).value().trim().to_owned();

        match self.port.read(cx).value().trim().parse::<u16>() {
            Ok(port) if port > 0 => proxy.port = port,
            _ if proxy.mode == ProxyMode::Custom => {
                self.error = Some("Enter a proxy port between 1 and 65535.".into());
                cx.notify();
                return;
            }
            _ => {}
        }

        if let Err(error) = proxy.validate() {
            self.error = Some(error.into());
        } else {
            let result = if proxy == cx.global::<Preferences>().request.proxy {
                Ok(())
            } else {
                preferences::update(cx, |preferences| preferences.request.proxy = proxy.clone())
            };

            match result {
                Ok(()) => {
                    self.draft = proxy;
                    self.error = None;
                }
                Err(error) => self.error = Some(format!("Could not save proxy settings: {error}")),
            }
        }

        cx.notify();
    }

    fn custom_fields(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let page = cx.entity().downgrade();
        let request_types = h_flex()
            .gap_4()
            .child(
                Checkbox::new("proxy-http")
                    .label("HTTP")
                    .checked(self.draft.http)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.draft.http = *checked;
                        this.save(cx);
                    })),
            )
            .child(
                Checkbox::new("proxy-https")
                    .label("HTTPS")
                    .checked(self.draft.https)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.draft.https = *checked;
                        this.save(cx);
                    })),
            );

        let server = h_flex()
            .w_full()
            .gap_2()
            .flex_wrap()
            .child(
                div().w(px(96.)).child(
                    Select::new(&self.protocol)
                        .accessibility_label("Proxy protocol")
                        .w_full(),
                ),
            )
            .child(
                div().flex_1().min_w(px(104.)).child(
                    Input::new(&self.host)
                        .aria_label("Proxy host")
                        .on_paste(move |clipboard, window, cx| {
                            page.update(cx, |page, cx| page.paste_proxy_url(clipboard, window, cx))
                                .unwrap_or(false)
                        })
                        .w_full(),
                ),
            )
            .child(
                div()
                    .w(px(80.))
                    .child(Input::new(&self.port).aria_label("Proxy port").w_full()),
            );

        v_flex()
            .id("proxy-custom-fields")
            .debug_selector(|| "proxy-custom-fields".into())
            .w_full()
            .child(row(
                "Use proxy for",
                "Other request types connect directly.",
                request_types,
                cx,
            ))
            .child(
                v_flex()
                    .py_4()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::MEDIUM).child("Proxy server"))
                    .child(server),
            )
            .child(row(
                "Proxy authentication",
                "Use a username and password with Basic authentication.",
                Switch::new("proxy-authentication")
                    .accessibility_label("Proxy authentication")
                    .checked(self.draft.authentication)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.draft.authentication = *checked;
                        this.save(cx);
                    })),
                cx,
            ))
            .when(self.draft.authentication, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .pb_4()
                        .flex_wrap()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(180.))
                                .gap_2()
                                .child("Username")
                                .child(Input::new(&self.username).aria_label("Proxy username").w_full()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(180.))
                                .gap_2()
                                .child("Password")
                                .child(
                                    Input::new(&self.password)
                                        .aria_label("Proxy password")
                                        .mask_toggle()
                                        .w_full(),
                                ),
                        ),
                )
            })
            .child(
                v_flex()
                    .py_4()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(div().font_weight(FontWeight::MEDIUM).child("Proxy bypass"))
                    .child(
                        div().text_color(cx.theme().muted_foreground).child(
                            "Comma-separated hosts, domains, or IP ranges that connect directly. Domains include subdomains.",
                        ),
                    )
                    .child(Input::new(&self.bypass).aria_label("Proxy bypass").w_full()),
            )
    }
}

fn row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .justify_between()
        .flex_wrap()
        .gap_4()
        .py_4()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_w(px(200.))
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(control)
}

impl Render for ProxySettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connection = div().w(px(190.)).flex_shrink_0().child(
            Select::new(&self.mode)
                .accessibility_label("Proxy connection")
                .w_full(),
        );

        v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(rems(1.625))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Proxy"),
                    )
                    .child(
                        div().text_color(cx.theme().muted_foreground).child(
                            "Choose how API requests connect to the network. Changes save automatically and apply to new requests.",
                        ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .child(row(
                        "Connection",
                        match self.draft.mode {
                            ProxyMode::System => "Use proxy settings from your system or environment.",
                            ProxyMode::Custom => "Send requests through the proxy configured below.",
                            ProxyMode::Disabled => "Connect directly without a proxy.",
                        },
                        connection,
                        cx,
                    ))
                    .when(self.draft.mode == ProxyMode::Custom, |this| {
                        this.child(self.custom_fields(cx))
                    }),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_color(cx.theme().danger).child(error))
            })
    }
}
