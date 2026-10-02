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
                    let body = json!([release("v0.1.30", &["request-eagle-update-other.json"])]);

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
