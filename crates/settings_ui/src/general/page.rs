use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, button::*, h_flex, progress::Progress, v_flex,
};
use gpui_kit::{prelude::*, *};

use updater::{UpdateStatus, Updater};

use super::request::RequestSettings;

pub(crate) struct GeneralSettings {
    updater: Entity<Updater>,
    request: Entity<RequestSettings>,
    _subscription: Subscription,
}

impl GeneralSettings {
    pub(crate) fn new(
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
            UpdateStatus::Downloading { version, .. } => {
                format!("Downloading version {version}…")
            }
            UpdateStatus::Verifying(version) => {
                format!("Verifying version {version}…")
            }
            UpdateStatus::Ready(version) => {
                format!("Version {version} is ready to install.")
            }
            UpdateStatus::Error(error) => error.clone(),
        };

        let available = matches!(status, UpdateStatus::Available(_));
        let downloading = matches!(status, UpdateStatus::Downloading { .. });
        let verifying = matches!(status, UpdateStatus::Verifying(_));
        let ready = matches!(status, UpdateStatus::Ready(_));
        let checking = matches!(status, UpdateStatus::Checking);
        let failed = matches!(status, UpdateStatus::Error(_));
        let download_progress = match status {
            UpdateStatus::Downloading {
                downloaded_bytes,
                total_bytes,
                ..
            } => Some((*downloaded_bytes, *total_bytes)),
            _ => None,
        };
        let percentage = download_progress.and_then(|(downloaded, total)| {
            total
                .filter(|total| *total > 0)
                .map(|total| (downloaded as f64 / total as f64 * 100.).clamp(0., 100.) as f32)
        });

        v_flex()
            .w_full()
            .max_w(crate::geometry::PAGE_WIDTH)
            .gap_6()
            .child(
                div()
                    .text_xl()
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
                    .child(if ready {
                        Button::new("relaunch-update")
                            .debug_selector(|| "relaunch-update".into())
                            .primary()
                            .label("Quit and relaunch")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updater.update(cx, |updater, cx| updater.relaunch(cx));
                            }))
                    } else if available || downloading || verifying {
                        Button::new("download-update")
                            .debug_selector(|| "download-update".into())
                            .primary()
                            .disabled(downloading || verifying)
                            .label(if downloading {
                                "Downloading…"
                            } else if verifying {
                                "Verifying…"
                            } else {
                                "Download update"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updater.update(cx, |updater, cx| updater.download(cx));
                            }))
                    } else {
                        Button::new("check-for-updates")
                            .debug_selector(|| "check-for-updates".into())
                            .outline()
                            .disabled(checking)
                            .label(if checking {
                                "Checking…"
                            } else {
                                "Check for updates"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updater.update(cx, |updater, cx| updater.check(cx));
                            }))
                    }),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .when(failed, |this| this.text_color(cx.theme().danger))
                                    .child(message),
                            )
                            .when_some(percentage, |this, percentage| {
                                this.child(div().flex_shrink_0().child(format!("{percentage:.0}%")))
                            }),
                    )
                    .when(checking || downloading || verifying, |this| {
                        this.child(
                            Progress::new("update-progress")
                                .accessibility_label(if verifying {
                                    "Verifying update"
                                } else if checking {
                                    "Checking for updates"
                                } else {
                                    "Update download progress"
                                })
                                .small()
                                .value(percentage.unwrap_or(0.))
                                .loading(percentage.is_none()),
                        )
                    })
                    .when_some(download_progress, |this, (downloaded, total)| {
                        this.child(match total.filter(|total| *total > 0) {
                            Some(total) => format!(
                                "{:.1} MB of {:.1} MB",
                                downloaded as f64 / 1_000_000.,
                                total as f64 / 1_000_000.
                            ),
                            None => format!("{:.1} MB downloaded", downloaded as f64 / 1_000_000.),
                        })
                    })
                    .when(available || downloading || verifying, |this| {
                        this.child("You can keep working. Relaunch when you're ready.")
                    })
                    .when(ready, |this| {
                        this.child(
                            "Download verified. Quit and relaunch to finish installing the update.",
                        )
                    }),
            )
            .child(self.request.clone())
    }
}
