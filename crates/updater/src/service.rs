#[cfg(target_os = "macos")]
use super::install;
use super::package::{self, MANIFEST};

use std::sync::Arc;

use gpui_kit::{
    App, AppContext, Context, Entity,
    http_client::{self, AsyncBody, HttpClient, Request, Response, StatusCode},
};
use preferences::{Preferences, UpdateTrack};
use semver::Version;
use serde::{Deserialize, de::DeserializeOwned};
use smol::io::AsyncReadExt;

const REPOSITORY: &str = "gregor-tokarev/request-eagle";

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub(super) url: String,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
    /// The track the latest check looked at.
    track: UpdateTrack,
    /// Counts checks, so the answer to a superseded one is dropped.
    checks: u64,
    pub(super) status: UpdateStatus,
    #[cfg(target_os = "macos")]
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

        self.checks += 1;
        let check = self.checks;
        let http_client = cx.http_client();
        let current_version = self.current_version;
        let track = self.track;

        cx.spawn(async move |this, cx| {
            let status = match check_for_update(http_client, current_version, track).await {
                Ok(Some(manifest)) => UpdateStatus::Available(manifest),
                Ok(None) => UpdateStatus::UpToDate,
                Err(error) => UpdateStatus::Error(error),
            };

            let _ = this.update(cx, |this, cx| {
                if this.checks == check {
                    this.set_status(status, cx);
                }
            });
        })
        .detach();
    }

    /// Check the newly chosen track. An update that is downloading finishes
    /// first, and one that is ready is dropped for the new track's answer.
    fn follow_track(&mut self, cx: &mut Context<Self>) {
        let track = cx.global::<Preferences>().update_track;

        if track == self.track
            || matches!(
                self.status,
                UpdateStatus::Downloading { .. } | UpdateStatus::Verifying(_)
            )
        {
            return;
        }

        self.track = track;

        #[cfg(target_os = "macos")]
        {
            self.prepared_update = None;
        }

        self.status = UpdateStatus::Idle;
        self.check(cx);
    }

    /// Downloads and verifies the update on macOS. Elsewhere the browser
    /// downloads the installer or package, and the system installs it.
    pub fn download(&mut self, cx: &mut Context<Self>) {
        let UpdateStatus::Available(manifest) = self.status.clone() else {
            return;
        };

        #[cfg(target_os = "macos")]
        self.download_in_app(manifest, cx);

        #[cfg(not(target_os = "macos"))]
        cx.open_url(&manifest.url);
    }

    #[cfg(target_os = "macos")]
    fn download_in_app(&mut self, manifest: UpdateManifest, cx: &mut Context<Self>) {
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

    /// Only a finished download stores a prepared update, so this does
    /// nothing until the status is `Ready`.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn relaunch(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if let Some(update) = self.prepared_update.take() {
            match update.launch_installer() {
                Ok(()) => cx.quit(),
                Err(error) => self.set_status(UpdateStatus::Error(error), cx),
            }
        }
    }
}

pub fn init(current_version: &'static str, cx: &mut App) -> Entity<Updater> {
    let updater = cx.new(|cx| {
        cx.observe_global::<Preferences>(Updater::follow_track)
            .detach();

        Updater {
            current_version,
            track: cx.global::<Preferences>().update_track,
            checks: 0,
            status: UpdateStatus::Idle,
            #[cfg(target_os = "macos")]
            prepared_update: None,
        }
    });

    // Builds from source cannot be replaced by a release package.
    if package::installed() {
        updater.update(cx, |updater, cx| updater.check(cx));
    }

    updater
}

/// A release as the GitHub API lists it.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Find this platform's manifest in the newest release. Nightly builds and
/// the stable releases promoted from them share one version sequence.
fn newest_manifest_url(releases: &[Release]) -> Option<&str> {
    releases
        .iter()
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            let manifest = release.assets.iter().find(|asset| asset.name == MANIFEST)?;

            Some((version, manifest.browser_download_url.as_str()))
        })
        .max_by(|(first, _), (second, _)| first.cmp(second))
        .map(|(_, url)| url)
}

async fn check_for_update(
    http_client: Arc<dyn HttpClient>,
    current_version: &str,
    track: UpdateTrack,
) -> Result<Option<UpdateManifest>, String> {
    let manifest_url = match track {
        // GitHub's latest release is the newest one not marked as a prerelease.
        UpdateTrack::Stable => {
            format!("https://github.com/{REPOSITORY}/releases/latest/download/{MANIFEST}")
        }
        UpdateTrack::Nightly => {
            let request = Request::get(format!(
                "https://api.github.com/repos/{REPOSITORY}/releases?per_page=30"
            ))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", format!("RequestEagle/{current_version}"))
            .body(AsyncBody::empty())
            .map_err(|error| format!("Could not check for updates: {error}"))?;

            let releases: Option<Vec<Release>> =
                get_json(http_client.send(request), "release list").await?;

            match releases.as_deref().and_then(newest_manifest_url) {
                Some(url) => url.to_owned(),
                None => return Ok(None),
            }
        }
    };

    // A track without a release for this platform has nothing to offer yet.
    let Some(manifest): Option<UpdateManifest> = get_json(
        http_client.get(&manifest_url, AsyncBody::empty(), true),
        "update manifest",
    )
    .await?
    else {
        return Ok(None);
    };

    let installed = Version::parse(current_version)
        .map_err(|error| format!("The installed version is invalid: {error}"))?;
    let released = Version::parse(manifest.version.trim_start_matches('v'))
        .map_err(|error| format!("The released version is invalid: {error}"))?;

    Ok((released > installed).then_some(manifest))
}

/// Decode a JSON document, or `None` when GitHub has no such file.
async fn get_json<T: DeserializeOwned>(
    response: impl Future<Output = http_client::Result<Response<AsyncBody>>>,
    name: &str,
) -> Result<Option<T>, String> {
    let mut response = response
        .await
        .map_err(|error| format!("Could not check for updates: {error}"))?;
    let status = response.status();

    if status == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    // Unauthenticated API requests are limited per network address.
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        return Err("GitHub is limiting update checks from this network. Try again later.".into());
    }

    let mut body = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .map_err(|error| format!("Could not read the {name}: {error}"))?;

    if !status.is_success() {
        let detail = String::from_utf8_lossy(&body).trim().to_string();

        return Err(if detail.is_empty() {
            format!("GitHub returned {status} for the {name}.")
        } else {
            detail
        });
    }

    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("The {name} is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use gpui_kit::http_client::{FakeHttpClient, Response};
    use preferences::UpdateTrack;
    use serde_json::{Value, json};

    use super::{MANIFEST, REPOSITORY, Release, check_for_update, newest_manifest_url};

    fn release(tag: &str, assets: &[&str]) -> Value {
        let assets: Vec<Value> = assets
            .iter()
            .map(|name| {
                json!({
                    "name": name,
                    "browser_download_url": format!("https://example.test/{tag}/{name}"),
                })
            })
            .collect();

        json!({ "tag_name": tag, "prerelease": true, "assets": assets })
    }

    fn manifest(version: &str) -> String {
        json!({ "version": version, "url": format!("https://example.test/{version}"), "sha256": "" })
            .to_string()
    }

    #[test]
    fn nightly_takes_the_newest_release_with_this_platforms_manifest() {
        let releases: Vec<Release> = serde_json::from_value(json!([
            release("v0.1.9", &[MANIFEST]),
            release("v0.1.11", &["request-eagle-update-other.json"]),
            release("v0.1.10", &["SHA256SUMS", MANIFEST]),
            release("nightly", &[MANIFEST]),
        ]))
        .unwrap();

        assert_eq!(
            newest_manifest_url(&releases),
            Some(format!("https://example.test/v0.1.10/{MANIFEST}").as_str())
        );
    }

    #[test]
    fn each_track_offers_its_newest_release() {
        smol::block_on(async {
            let http = FakeHttpClient::create(|request| {
                let url = request.uri().to_string();

                async move {
                    let body = if url
                        == format!(
                            "https://github.com/{REPOSITORY}/releases/latest/download/{MANIFEST}"
                        ) {
                        manifest("0.1.20")
                    } else if url.starts_with(&format!(
                        "https://api.github.com/repos/{REPOSITORY}/releases"
                    )) {
                        json!([
                            release("v0.1.20", &[MANIFEST]),
                            release("v0.1.22", &[MANIFEST])
                        ])
                        .to_string()
                    } else if url == format!("https://example.test/v0.1.22/{MANIFEST}") {
                        manifest("0.1.22")
                    } else {
                        panic!("Unexpected request to {url}");
                    };

                    Ok(Response::builder().status(200).body(body.into()).unwrap())
                }
            });

            let stable = check_for_update(http.clone(), "0.1.19", UpdateTrack::Stable).await;
            let nightly = check_for_update(http.clone(), "0.1.19", UpdateTrack::Nightly).await;

            assert_eq!(stable.unwrap().unwrap().version, "0.1.20");
            assert_eq!(nightly.unwrap().unwrap().version, "0.1.22");

            // A nightly build stays ahead of stable until a newer one is promoted.
            let back_on_stable = check_for_update(http, "0.1.22", UpdateTrack::Stable).await;

            assert!(back_on_stable.unwrap().is_none());
        });
    }

    #[test]
    fn a_track_without_a_release_for_this_platform_has_no_update() {
        smol::block_on(async {
            let http = FakeHttpClient::create(|request| {
                let listing = request.uri().host() == Some("api.github.com");

                async move {
                    Ok(if listing {
                        let body =
                            json!([release("v0.1.30", &["request-eagle-update-other.json"])]);

                        Response::builder()
                            .status(200)
                            .body(body.to_string().into())
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(404)
                            .body("Not Found".into())
                            .unwrap()
                    })
                }
            });

            for track in [UpdateTrack::Stable, UpdateTrack::Nightly] {
                let update = check_for_update(http.clone(), "0.1.19", track).await;

                assert!(update.unwrap().is_none());
            }
        });
    }

    #[test]
    fn rate_limits_explain_themselves() {
        smol::block_on(async {
            let http = FakeHttpClient::create(|_| async {
                Ok(Response::builder()
                    .status(403)
                    .body(r#"{"message":"API rate limit exceeded"}"#.into())
                    .unwrap())
            });

            let error = check_for_update(http, "0.1.19", UpdateTrack::Nightly)
                .await
                .unwrap_err();

            assert!(error.contains("Try again later"));
        });
    }
}
