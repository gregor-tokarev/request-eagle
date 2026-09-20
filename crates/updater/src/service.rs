use super::install;

use std::sync::Arc;

use gpui_kit::{
    App, AppContext, Context, Entity,
    http_client::{AsyncBody, HttpClient},
};
use semver::Version;
use serde::Deserialize;
use smol::io::AsyncReadExt;

const UPDATE_MANIFEST_URL: &str = "https://github.com/gregor-tokarev/request-eagle/releases/latest/download/request-eagle-update.json";

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub(super) url: String,
    pub(super) sha256: String,
}

#[derive(Clone, Debug)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateManifest),
    Downloading {
        version: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Verifying(String),
    Ready(String),
    Error(String),
}

pub struct Updater {
    pub(super) current_version: &'static str,
    pub(super) status: UpdateStatus,
    pub(super) prepared_update: Option<install::PreparedUpdate>,
}

impl Updater {
    pub fn current_version(&self) -> &str {
        self.current_version
    }

    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    fn set_status(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
        self.status = status;

        cx.notify();
    }

    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.status,
            UpdateStatus::Checking
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Verifying(_)
                | UpdateStatus::Ready(_)
        ) {
            return;
        }

        self.set_status(UpdateStatus::Checking, cx);

        let http_client = cx.http_client();
        let current_version = self.current_version;

        cx.spawn(async move |this, cx| {
            let status = match check_for_update(http_client, current_version).await {
                Ok(Some(manifest)) => UpdateStatus::Available(manifest),
                Ok(None) => UpdateStatus::UpToDate,
                Err(error) => UpdateStatus::Error(error),
            };

            let _ = this.update(cx, |this, cx| this.set_status(status, cx));
        })
        .detach();
    }

    pub fn download(&mut self, cx: &mut Context<Self>) {
        let UpdateStatus::Available(manifest) = self.status.clone() else {
            return;
        };

        let version = manifest.version.clone();

        self.set_status(
            UpdateStatus::Downloading {
                version: version.clone(),
                downloaded_bytes: 0,
                total_bytes: None,
            },
            cx,
        );

        let http_client = cx.http_client();
        let (progress, updates) = smol::channel::unbounded();
        let task = cx.background_executor().spawn(async move {
            install::download_and_prepare_update(&manifest, http_client, progress).await
        });

        cx.spawn(async move |this, cx| {
            while let Ok(status) = updates.recv().await {
                if this
                    .update(cx, |this, cx| this.set_status(status, cx))
                    .is_err()
                {
                    return;
                }
            }

            let result = task.await;

            let _ = this.update(cx, |this, cx| match result {
                Ok(update) => {
                    this.prepared_update = Some(update);
                    this.set_status(UpdateStatus::Ready(version), cx);
                }
                Err(error) => this.set_status(UpdateStatus::Error(error), cx),
            });
        })
        .detach();
    }

    pub fn relaunch(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.status, UpdateStatus::Ready(_)) {
            return;
        }

        let Some(update) = self.prepared_update.take() else {
            return;
        };

        match update.launch_installer() {
            Ok(()) => cx.quit(),
            Err(error) => self.set_status(UpdateStatus::Error(error), cx),
        }
    }
}

pub fn init(current_version: &'static str, cx: &mut App) -> Entity<Updater> {
    let updater = cx.new(|_| Updater {
        current_version,
        status: UpdateStatus::Idle,
        prepared_update: None,
    });

    // Bare cargo binaries cannot be replaced by the app-bundle installer.
    if install::current_app_bundle().is_ok() {
        updater.update(cx, |updater, cx| updater.check(cx));
    }

    updater
}

pub(super) async fn check_for_update(
    http_client: Arc<dyn HttpClient>,
    current_version: &str,
) -> Result<Option<UpdateManifest>, String> {
    let mut response = http_client
        .get(UPDATE_MANIFEST_URL, AsyncBody::empty(), true)
        .await
        .map_err(|error| format!("Could not check for updates: {error}"))?;
    let status = response.status();

    let mut body = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .map_err(|error| format!("Could not read the update manifest: {error}"))?;

    if !status.is_success() {
        let detail = String::from_utf8_lossy(&body).trim().to_string();

        return Err(if detail.is_empty() {
            format!("GitHub returned {status} for the update manifest.")
        } else {
            detail
        });
    }

    let manifest: UpdateManifest = serde_json::from_slice(&body)
        .map_err(|error| format!("The update manifest is invalid: {error}"))?;

    let installed = Version::parse(current_version)
        .map_err(|error| format!("The installed version is invalid: {error}"))?;
    let released = Version::parse(manifest.version.trim_start_matches('v'))
        .map_err(|error| format!("The released version is invalid: {error}"))?;

    Ok((released > installed).then_some(manifest))
}
