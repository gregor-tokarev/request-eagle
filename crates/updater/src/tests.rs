use gpui_kit::{
    AppContext as _, TestAppContext,
    http_client::{FakeHttpClient, Response},
};

use super::{UpdateStatus, Updater, check_for_update};

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

fn manifest(version: &str) -> String {
    serde_json::json!({
        "version": version,
        "url": "https://example.test/RequestEagle.zip",
        "sha256": "abc123",
    })
    .to_string()
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
            check_for_update(http, "1.2.3").await.unwrap().is_some(),
            available,
            "release {version}"
        );
    }

    for body in ["not json".to_owned(), manifest("invalid-version")] {
        let http = FakeHttpClient::create(move |_| {
            let body = body.clone();
            async { Ok(Response::builder().status(200).body(body.into()).unwrap()) }
        });

        assert!(check_for_update(http, "1.2.3").await.is_err());
    }
}
