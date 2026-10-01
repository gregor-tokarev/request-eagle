use std::sync::{Arc, Mutex};

use gpui_kit::{
    AppContext as _, TestAppContext,
    http_client::{FakeHttpClient, Response},
};
use preferences::UpdateChannel;
use serde_json::json;

use crate::service::{Release, check_for_update, newest_manifest_url};
use crate::{UpdateStatus, Updater};

const STABLE_MANIFEST_URL: &str = "https://github.com/gregor-tokarev/request-eagle/releases/latest/download/request-eagle-update.json";
const RELEASES_URL: &str =
    "https://api.github.com/repos/gregor-tokarev/request-eagle/releases?per_page=20";

#[gpui_kit::test]
fn checks_against_the_application_version(cx: &mut TestAppContext) {
    let http = FakeHttpClient::create(|_| async {
        Ok(Response::builder()
            .status(200)
            .body(manifest("1.2.3").into())
            .unwrap())
    });

    let updater = cx.update(|cx| {
        cx.set_http_client(http);

        super::init("1.2.3", cx)
    });

    updater.update(cx, |updater, cx| updater.check(cx));
    cx.run_until_parked();

    cx.read(|cx| {
        assert_eq!(updater.read(cx).current_version(), "1.2.3");
        assert!(matches!(updater.read(cx).status(), UpdateStatus::UpToDate));
    });
}

#[gpui_kit::test]
fn pending_updates_cannot_be_interrupted_or_restarted(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_http_client(FakeHttpClient::create(|_| async {
            panic!("A pending update must not start another HTTP request")
        }));
    });

    for status in [
        UpdateStatus::Downloading {
            version: "99.0.0".into(),
            downloaded_bytes: 100,
            total_bytes: Some(200),
        },
        UpdateStatus::Verifying("99.0.0".into()),
        UpdateStatus::Ready("99.0.0".into()),
    ] {
        let before = format!("{status:?}");
        let updater = cx.new(|_| Updater {
            current_version: "1.2.3",
            status,
            prepared_update: None,
        });

        updater.update(cx, |updater, cx| {
            updater.check(cx);
            updater.download(cx);
        });
        cx.run_until_parked();

        cx.read(|cx| assert_eq!(format!("{:?}", updater.read(cx).status()), before));
    }
}

#[gpui_kit::test]
fn a_check_started_before_the_channel_changed_checks_again(cx: &mut TestAppContext) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (respond, wait) = smol::channel::bounded::<()>(1);
    let http = FakeHttpClient::create({
        let requests = requests.clone();

        move |request| {
            let uri = request.uri().to_string();
            requests.lock().unwrap().push(uri.clone());
            let wait = wait.clone();

            async move {
                let body = match uri.as_str() {
                    STABLE_MANIFEST_URL => {
                        wait.recv().await.unwrap();
                        manifest("1.2.3")
                    }
                    RELEASES_URL => json!([release("v99.0.0", false, true)]).to_string(),
                    _ => manifest("99.0.0"),
                };

                Ok(Response::builder().status(200).body(body.into()).unwrap())
            }
        }
    });

    cx.update(|cx| {
        preferences::init(cx);
        cx.set_http_client(http);
    });

    let updater = cx.update(|cx| super::init("1.2.3", cx));
    updater.update(cx, |updater, cx| updater.check(cx));
    cx.run_until_parked();

    cx.update(|cx| {
        preferences::update(cx, |preferences| {
            preferences.update_channel = UpdateChannel::Daily;
        })
        .unwrap();
    });

    // The in-flight stable check must not decide the daily channel's result.
    updater.update(cx, |updater, cx| updater.check(cx));
    respond.try_send(()).unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Available(manifest) if manifest.version == "99.0.0"
        ));
    });
    assert_eq!(
        requests.lock().unwrap()[..2],
        [STABLE_MANIFEST_URL, RELEASES_URL]
    );
}

fn manifest(version: &str) -> String {
    serde_json::json!({
        "version": version,
        "url": "https://example.test/RequestEagle.zip",
        "sha256": "abc123",
    })
    .to_string()
}

fn release(tag: &str, draft: bool, has_manifest: bool) -> serde_json::Value {
    let mut assets = vec![json!({
        "name": format!("RequestEagle-{}-arm64.zip", tag.trim_start_matches('v')),
        "browser_download_url": "https://example.test/RequestEagle.zip",
    })];

    if has_manifest {
        assets.push(json!({
            "name": "request-eagle-update.json",
            "browser_download_url": manifest_url(tag),
        }));
    }

    json!({
        "tag_name": tag,
        "draft": draft,
        "prerelease": true,
        "assets": assets,
    })
}

fn manifest_url(tag: &str) -> String {
    format!(
        "https://github.com/gregor-tokarev/request-eagle/releases/download/{tag}/request-eagle-update.json"
    )
}

#[gpui_kit::test]
async fn update_check_handles_version_boundaries_and_invalid_responses() {
    for (version, available) in [
        ("99.0.0", true),
        ("v99.0.0", true),
        ("1.2.3", false),
        ("0.0.1", false),
    ] {
        let http = FakeHttpClient::create(move |_| async move {
            Ok(Response::builder()
                .status(200)
                .body(manifest(version).into())
                .unwrap())
        });

        assert_eq!(
            check_for_update(http, "1.2.3", UpdateChannel::Stable)
                .await
                .unwrap()
                .is_some(),
            available,
            "release {version}"
        );
    }

    for body in ["not json".to_owned(), manifest("invalid-version")] {
        let http = FakeHttpClient::create(move |_| {
            let body = body.clone();
            async { Ok(Response::builder().status(200).body(body.into()).unwrap()) }
        });

        assert!(
            check_for_update(http, "1.2.3", UpdateChannel::Stable)
                .await
                .is_err()
        );
    }
}

#[test]
fn daily_builds_follow_the_highest_published_release_with_a_manifest() {
    let newest = |releases: serde_json::Value| {
        let releases: Vec<Release> = serde_json::from_value(releases).unwrap();

        newest_manifest_url(&releases).map(str::to_owned)
    };

    // Versions compare numerically, and a newer stable release beats an
    // older pre-release wherever it appears in the list.
    let mut stable = release("v0.1.20", false, true);
    stable["prerelease"] = false.into();
    assert_eq!(
        newest(json!([
            release("v0.1.19", false, true),
            stable,
            release("v0.1.9", false, true),
        ])),
        Some(manifest_url("v0.1.20"))
    );

    // Drafts, releases still missing their manifest and other tags are skipped.
    assert_eq!(
        newest(json!([
            release("v0.1.23", true, true),
            release("v0.1.22", false, false),
            release("nightly", false, true),
            release("v0.1.21", false, true),
        ])),
        Some(manifest_url("v0.1.21"))
    );

    assert_eq!(newest(json!([release("v0.1.22", false, false)])), None);
}

#[gpui_kit::test]
async fn daily_checks_ask_github_for_pre_releases_and_fetch_their_manifest() {
    let http = FakeHttpClient::create(|request| async move {
        let body = match request.uri().to_string() {
            uri if uri == RELEASES_URL => {
                assert_eq!(request.headers()["accept"], "application/vnd.github+json");
                assert_eq!(request.headers()["user-agent"], "RequestEagle/1.2.3");

                json!([
                    release("v1.2.4", false, true),
                    release("v1.2.3", false, true)
                ])
                .to_string()
            }
            uri if uri == manifest_url("v1.2.4") => manifest("1.2.4"),
            uri => panic!("Unexpected request to {uri}"),
        };

        Ok(Response::builder().status(200).body(body.into()).unwrap())
    });

    let update = check_for_update(http, "1.2.3", UpdateChannel::Daily)
        .await
        .unwrap();

    assert_eq!(update.unwrap().version, "1.2.4");
}

#[gpui_kit::test]
async fn daily_checks_explain_github_failures() {
    for (status, body, error) in [
        (
            403_u16,
            r#"{"message":"API rate limit exceeded","documentation_url":"https://docs.github.com"}"#,
            "API rate limit exceeded",
        ),
        (200, "[]", "No published release has an update manifest."),
    ] {
        let http = FakeHttpClient::create(move |_| async move {
            Ok(Response::builder()
                .status(status)
                .body(body.into())
                .unwrap())
        });

        assert_eq!(
            check_for_update(http, "1.2.3", UpdateChannel::Daily)
                .await
                .unwrap_err(),
            error
        );
    }
}
