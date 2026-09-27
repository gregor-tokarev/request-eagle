use crate::CliAccess;
use gpui_kit::component::{ActiveTheme, Disableable, button::*, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use updater::{CliInstaller, CliStatus};

pub(super) struct CliSettings {
    installer: Entity<CliInstaller>,
    version: String,
    _subscription: Subscription,
    _access_subscription: Subscription,
}

impl CliSettings {
    pub(super) fn new(version: &str, cx: &mut Context<Self>) -> Self {
        CliAccess::init(cx);
        let access_subscription = cx.observe_global::<CliAccess>(|_, cx| cx.notify());
        let installer = cx.new(|cx| CliInstaller::new(version, cx));
        let subscription = cx.observe(&installer, |_, _, cx| cx.notify());
        Self {
            installer,
            version: version.into(),
            _subscription: subscription,
            _access_subscription: access_subscription,
        }
    }
}

impl Render for CliSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let access = cx.global::<CliAccess>();
        let enabled = access.token.is_some();
        let access_error = access.error.clone();
        let status = self.installer.read(cx).status();
        let installing = matches!(status, CliStatus::Installing);
        let current =
            matches!(status, CliStatus::Installed { version, .. } if version == &self.version);
        let failed = matches!(status, CliStatus::Error(_));
        let message = match status {
            CliStatus::NotInstalled => {
                "Optional download. The CLI connects to Request Eagle while the app is running."
                    .into()
            }
            CliStatus::Installing => "Downloading and verifying the CLI…".into(),
            CliStatus::Installed { version, path } => {
                format!("Version {version} installed at {}", path.display())
            }
            CliStatus::Error(error) => error.clone(),
        };
        v_flex().debug_selector(|| "cli-install-section".into()).w_full().gap_3()
            .child(h_flex().w_full().justify_between().flex_wrap().gap_4()
                .child(v_flex().flex_1().min_w_0().gap_2()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Request Eagle CLI"))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Let AI agents work with your collections, request drafts, and responses from the terminal.")))
                .child(Button::new("install-cli").debug_selector(|| "install-cli".into()).outline()
                    .label(if installing { "Installing…" } else if current { "Installed" } else if matches!(status, CliStatus::Installed { .. }) { "Update CLI" } else { "Install CLI" })
                    .disabled(installing || current)
                    .on_click(cx.listener(|this, _, _, cx| this.installer.update(cx, |installer, cx| installer.install(cx))))))
            .child(div().text_sm().text_color(if failed { cx.theme().danger } else { cx.theme().muted_foreground }).child(message))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(if enabled {
                "CLI access is enabled for this app session. Share the session token only with agents you trust. Disable access to revoke it."
            } else {
                "CLI access is off. Enabling it lets authorized agents read and change requests, responses, and settings until you disable it or quit the app."
            }))
            .child(h_flex().flex_wrap().gap_2()
                .child(Button::new("toggle-cli-access").debug_selector(|| "toggle-cli-access".into())
                    .label(if enabled { "Disable CLI access" } else { "Enable CLI access" })
                    .on_click(move |_, _, cx| if enabled { CliAccess::disable(cx) } else { CliAccess::enable(cx) }))
                .when(enabled, |this| this.child(Button::new("copy-cli-session").label("Copy session command")
                    .on_click(|_, _, cx| {
                        if let Some(token) = &cx.global::<CliAccess>().token {
                            cx.write_to_clipboard(ClipboardItem::new_string(format!("export REQUEST_EAGLE_CLI_TOKEN='{token}'")));
                        }
                    }))))
            .when_some(access_error, |this, error| this.child(div().text_sm().text_color(cx.theme().danger).child(error)))
            .when(matches!(status, CliStatus::Installed { .. }), |this| this
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Add ~/.request-eagle/bin to your shell's PATH, then run request-eagle-cli schema to get started."))
                .child(Button::new("copy-cli-path").label("Copy PATH command").on_click(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("export PATH=\"$HOME/.request-eagle/bin:$PATH\"".into())))))
    }
}
