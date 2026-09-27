use gpui_kit::component::{ActiveTheme, Disableable, button::*, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use updater::{CliInstaller, CliStatus};

pub(super) struct CliSettings {
    installer: Entity<CliInstaller>,
    version: String,
    _subscription: Subscription,
}

impl CliSettings {
    pub(super) fn new(version: &str, cx: &mut Context<Self>) -> Self {
        let installer = cx.new(|cx| CliInstaller::new(version, cx));
        let subscription = cx.observe(&installer, |_, _, cx| cx.notify());
        Self {
            installer,
            version: version.into(),
            _subscription: subscription,
        }
    }
}

impl Render for CliSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .when(matches!(status, CliStatus::Installed { .. }), |this| this
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Add ~/.request-eagle/bin to your shell's PATH, then run request-eagle-cli schema to get started."))
                .child(Button::new("copy-cli-path").label("Copy PATH command").on_click(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("export PATH=\"$HOME/.request-eagle/bin:$PATH\"".into())))))
    }
}
