use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, IndexPath, Sizable as _,
    button::*,
    h_flex,
    progress::Progress,
    select::{SearchableVec, Select, SelectEvent, SelectState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{prelude::*, *};

use preferences::{Preferences, UpdateTrack};
use updater::{INSTALLS_IN_APP, UpdateStatus, Updater};

use super::request::RequestSettings;
use crate::layout::{self, row, section};

const UPDATE_TRACKS: [(UpdateTrack, &str); 2] = [
    (UpdateTrack::Stable, "Stable"),
    (UpdateTrack::Nightly, "Nightly"),
];

type TrackList = SearchableVec<SharedString>;

pub(crate) struct GeneralSettings {
    updater: Entity<Updater>,
    update_track: Entity<SelectState<TrackList>>,
    request: Entity<RequestSettings>,
    update_track_error: Option<String>,
    error: Option<String>,
    usage_data_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl GeneralSettings {
    pub(crate) fn new(
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&updater, |_, _, cx| cx.notify());
        let preferences = cx.observe_global::<Preferences>(|_, cx| cx.notify());

        let track = cx.global::<Preferences>().update_track;
        let selected = UPDATE_TRACKS
            .iter()
            .position(|(candidate, _)| *candidate == track)
            .unwrap_or(0);

        let update_track = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    UPDATE_TRACKS
                        .iter()
                        .map(|(_, label)| SharedString::from(*label))
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(selected)),
                window,
                cx,
            )
        });

        // The updater follows the saved preference and checks the new track.
        let track_subscription = cx.subscribe(
            &update_track,
            |this, _, event: &SelectEvent<TrackList>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some(&(track, _)) = UPDATE_TRACKS
                        .iter()
                        .find(|(_, candidate)| *candidate == label.as_ref())
                {
                    this.update_track_error = preferences::update(cx, |preferences| {
                        preferences.update_track = track;
                    })
                    .err()
                    .map(|error| format!("Could not save the update track: {error}"));

                    cx.notify();
                }
            },
        );

        Self {
            updater,
            update_track,
            request: cx.new(|cx| RequestSettings::new(window, cx)),
            update_track_error: None,
            error: None,
            usage_data_error: None,
            _subscriptions: vec![subscription, preferences, track_subscription],
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

        let update_button = if ready {
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
                .when(!INSTALLS_IN_APP, |this| this.icon(IconName::ExternalLink))
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
        };

        let update_status = v_flex()
            .w_full()
            .gap_2()
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
            .when(available && !INSTALLS_IN_APP, |this| {
                this.child("The update downloads in your browser. Open it to install.")
            })
            .when(
                INSTALLS_IN_APP && (available || downloading || verifying),
                |this| this.child("You can keep working. Relaunch when you're ready."),
            )
            .when(ready, |this| {
                this.child("Download verified. Quit and relaunch to finish installing the update.")
            });

        let vim_mode = div()
            .debug_selector(|| "vim-mode".into())
            .flex_shrink_0()
            .child(
                Switch::new("vim-mode")
                    .accessibility_label("Vim mode")
                    .checked(cx.global::<Preferences>().vim_mode)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.error = preferences::update(cx, |preferences| {
                            preferences.vim_mode = *checked;
                        })
                        .err()
                        .map(|error| format!("Could not save Vim mode: {error}"));

                        cx.notify();
                    })),
            );

        let usage_data = div()
            .debug_selector(|| "share-usage-data".into())
            .flex_shrink_0()
            .child(
                Switch::new("share-usage-data")
                    .accessibility_label("Share usage data")
                    .checked(cx.global::<Preferences>().share_usage_data)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.usage_data_error = preferences::update(cx, |preferences| {
                            preferences.share_usage_data = *checked;
                        })
                        .err()
                        .map(|error| format!("Could not save the usage data setting: {error}"));

                        // Turning usage data off stops it until the app quits,
                        // even when the choice could not be saved.
                        if this.usage_data_error.is_some() && !*checked {
                            cx.global_mut::<Preferences>().share_usage_data = false;
                        }

                        cx.notify();
                    })),
            );

        v_flex()
            .w_full()
            .max_w(layout::PAGE_WIDTH)
            .gap_6()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("General"),
            )
            .child(
                section("Updates")
                    .child(row(
                        format!("Request Eagle {}", updater.current_version()),
                        update_status,
                        div().flex_shrink_0().child(update_button),
                        cx,
                    ))
                    .child(row(
                        "Update track",
                        "Use stable releases or nightly builds. Switch back anytime.",
                        div().w_40().flex_shrink_0().child(
                            Select::new(&self.update_track)
                                .accessibility_label("Update track")
                                .disabled(downloading || verifying)
                                .w_full(),
                        ),
                        cx,
                    ))
                    .when_some(self.update_track_error.clone(), |this, error| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    }),
            )
            .child(
                section("Editor")
                    .child(
                        row(
                            "Vim mode",
                            "Use Vim keybindings in request body and script editors.",
                            vim_mode,
                            cx,
                        )
                        .debug_selector(|| "vim-mode-row".into()),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    }),
            )
            .child(self.request.clone())
            .child(
                section("Agent CLI")
                    .debug_selector(|| "cli-install-section".into())
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .py_4()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(div().font_weight(FontWeight::MEDIUM).child("Request Eagle CLI"))
                            .child(div().text_color(cx.theme().muted_foreground).child(
                                "Let AI agents manage saved collections, run requests, and edit settings from the terminal. The CLI works independently of the app and is a separate download for macOS and Linux.",
                            ))
                            .child(
                                h_flex()
                                    .pt_2()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("download-cli")
                                            .outline()
                                            .label("Download CLI")
                                            .icon(IconName::ExternalLink)
                                            .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/releases/latest")),
                                    )
                                    .child(
                                        Button::new("cli-instructions")
                                            .ghost()
                                            .label("Installation instructions")
                                            .icon(IconName::ExternalLink)
                                            .on_click(|_, _, cx| cx.open_url("https://github.com/gregor-tokarev/request-eagle/blob/main/docs/cli.md#install")),
                                    ),
                            ),
                    ),
            )
            .child(
                section("Privacy")
                    .child(row(
                        "Share usage data",
                        "Send anonymous statistics about which features you use, to help improve Request Eagle. Your requests, responses and collections are never sent.",
                        usage_data,
                        cx,
                    ))
                    .when_some(self.usage_data_error.clone(), |this, error| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    }),
            )
    }
}
