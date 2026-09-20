use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use gpui_kit::{
    Modifiers, TestAppContext,
    http_client::{FakeHttpClient, Response},
};

use super::GeneralSettings;
use updater::UpdateStatus;

fn manifest(version: &str) -> String {
    serde_json::json!({
        "version": version,
        "url": "https://example.test/RequestEagle.zip",
        "sha256": "abc123",
    })
    .to_string()
}

#[gpui_kit::test]
fn checking_survives_closing_settings_and_does_not_open_a_window(cx: &mut TestAppContext) {
    let requests = Arc::new(AtomicUsize::new(0));
    let (respond, wait) = smol::channel::bounded::<()>(1);
    let http = FakeHttpClient::create({
        let requests = requests.clone();
        move |request| {
            assert_eq!(
                request.uri().to_string(),
                "https://github.com/gregor-tokarev/request-eagle/releases/latest/download/request-eagle-update.json"
            );
            requests.fetch_add(1, Ordering::SeqCst);
            let wait = wait.clone();

            async move {
                wait.recv().await.unwrap();

                Ok(Response::builder()
                    .status(200)
                    .body(manifest("99.0.0").into())
                    .unwrap())
            }
        }
    });

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_http_client(http);
    });

    let updater = cx.update(|cx| updater::init("1.2.3", cx));

    let (page, view) =
        cx.add_window_view(|window, cx| GeneralSettings::new(updater.clone(), window, cx));

    let check = view.debug_bounds("check-for-updates").unwrap();
    view.simulate_click(check.center(), Modifiers::default());

    // Both the button and another caller must coalesce onto the in-flight check.
    view.simulate_click(check.center(), Modifiers::default());
    updater.update(view, |updater, cx| updater.check(cx));
    view.run_until_parked();

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(matches!(
        updater.read_with(view, |updater, _| updater.status().clone()),
        UpdateStatus::Checking
    ));

    view.update(|window, _| window.remove_window());
    drop(page);

    respond.try_send(()).unwrap();
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            cx.windows().is_empty(),
            "A completed background check must not open UI"
        );
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Available(manifest) if manifest.version == "99.0.0"
        ));
    });

    let (_, view) =
        cx.add_window_view(|window, cx| GeneralSettings::new(updater.clone(), window, cx));
    assert!(
        view.debug_bounds("download-update").is_some(),
        "Reopened General must offer the completed update"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

#[gpui_kit::test]
fn failed_check_can_be_retried_from_general(cx: &mut TestAppContext) {
    let requests = Arc::new(AtomicUsize::new(0));
    let http = FakeHttpClient::create({
        let requests = requests.clone();
        move |_| {
            let attempt = requests.fetch_add(1, Ordering::SeqCst);

            async move {
                Ok(if attempt == 0 {
                    Response::builder()
                        .status(503)
                        .body("Release service unavailable".into())
                        .unwrap()
                } else {
                    Response::builder()
                        .status(200)
                        .body(manifest("99.0.0").into())
                        .unwrap()
                })
            }
        }
    });

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_http_client(http);
    });

    let updater = cx.update(|cx| updater::init("1.2.3", cx));

    let (_, view) =
        cx.add_window_view(|window, cx| GeneralSettings::new(updater.clone(), window, cx));

    let check = view.debug_bounds("check-for-updates").unwrap();
    view.simulate_click(check.center(), Modifiers::default());

    view.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Error(error) if error == "Release service unavailable"
        ));
    });

    let retry = view.debug_bounds("check-for-updates").unwrap();
    view.simulate_click(retry.center(), Modifiers::default());

    view.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Available(_)
        ));
    });

    let download = view
        .debug_bounds("download-update")
        .expect("General must update its button when the check completes");

    assert!(view.debug_bounds("relaunch-update").is_none());

    // Cargo test binaries have no app bundle. Exercise the real download button
    // and failure state without downloading or replacing an app.
    view.simulate_click(download.center(), Modifiers::default());

    view.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Error(error) if error.contains("only available from Request Eagle.app")
        ));
    });

    assert!(view.debug_bounds("check-for-updates").is_some());
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}
