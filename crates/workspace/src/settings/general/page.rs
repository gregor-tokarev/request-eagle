use gpui_kit::component::{ActiveTheme as _, Disableable as _, button::*, h_flex, v_flex};
use gpui_kit::{prelude::*, *};

use updater::{UpdateStatus, Updater};

use super::request::RequestSettings;

pub(in crate::settings) struct GeneralSettings {
    updater: Entity<Updater>,
    request: Entity<RequestSettings>,
    _subscription: Subscription,
}

impl GeneralSettings {
    pub(in crate::settings) fn new(
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&updater, |_, _, cx| cx.notify());

        Self {
            updater,
            request: cx.new(|cx| RequestSettings::new(window, cx)),
            _subscription: subscription,
        }
    }
}

impl Render for GeneralSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updater = self.updater.read(cx);
        let status = updater.status();
        let message = match status {
            UpdateStatus::Idle => "Check for a new version of Request Eagle.".to_owned(),
            UpdateStatus::Checking => "Checking for updates…".to_owned(),
            UpdateStatus::UpToDate => "You're up to date.".to_owned(),
            UpdateStatus::Available(manifest) => {
                format!("Version {} is available.", manifest.version)
            }
            UpdateStatus::Installing(version) => {
                format!("Downloading and verifying version {version}…")
            }
            UpdateStatus::Error(error) => error.clone(),
        };

        let available = matches!(status, UpdateStatus::Available(_));
        let installing = matches!(status, UpdateStatus::Installing(_));
        let busy = installing || matches!(status, UpdateStatus::Checking);
        let failed = matches!(status, UpdateStatus::Error(_));

        v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_6()
            .child(
                div()
                    .text_size(rems(1.625))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("General"),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().font_weight(FontWeight::MEDIUM).child("Request Eagle"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("Version {}", updater.current_version())),
                            ),
                    )
                    .child(
                        Button::new("update-action")
                            .debug_selector(move || {
                                if available {
                                    "install-update"
                                } else {
                                    "check-for-updates"
                                }
                                .into()
                            })
                            .outline()
                            .when(available, |button| button.primary())
                            .disabled(busy)
                            .label(if available {
                                "Install and relaunch"
                            } else if installing {
                                "Installing…"
                            } else if busy {
                                "Checking…"
                            } else {
                                "Check for updates"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.updater.update(cx, |updater, cx| {
                                    if available {
                                        updater.install(cx);
                                    } else {
                                        updater.check(cx);
                                    }
                                });
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(if failed {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(message)
                    .when(installing, |this| {
                        this.child("Request Eagle will relaunch when the update is installed.")
                    }),
            )
            .child(self.request.clone())
    }
}
