use std::sync::{
    Arc, Mutex,
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
async fn vim_toggle_saves_and_keeps_the_previous_value_if_saving_fails(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(gpui_kit::init);
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await
        .unwrap();
    let updater = cx.update(|cx| updater::init("1.2.3", cx));
    let (_, view) = cx.add_window_view(|window, cx| GeneralSettings::new(updater, window, cx));
    let toggle = view.debug_bounds("vim-mode").unwrap();

    view.read(|cx| assert!(!cx.global::<preferences::Preferences>().vim_mode));
    view.simulate_click(toggle.center(), Modifiers::default());
    view.read(|cx| assert!(cx.global::<preferences::Preferences>().vim_mode));
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["vim_mode"], true);

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    view.simulate_click(toggle.center(), Modifiers::default());
    view.read(|cx| assert!(cx.global::<preferences::Preferences>().vim_mode));
    assert!(path.is_dir());

    std::fs::remove_dir(&path).unwrap();
    view.simulate_keystrokes("space");
    view.simulate_event(gpui_kit::KeyUpEvent {
        keystroke: gpui_kit::Keystroke::parse("space").unwrap(),
    });
    view.read(|cx| assert!(!cx.global::<preferences::Preferences>().vim_mode));
}

#[gpui_kit::test]
async fn daily_builds_toggle_saves_the_channel_and_checks_it(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let http = FakeHttpClient::create({
        let requests = requests.clone();

        move |request| {
            let uri = request.uri().to_string();
            requests.lock().unwrap().push(uri.clone());

            async move {
                let body = if uri.starts_with("https://api.github.com/") {
                    serde_json::json!([{
                        "tag_name": "v99.0.0",
                        "draft": false,
                        "prerelease": true,
                        "assets": [{
                            "name": "request-eagle-update.json",
                            "browser_download_url": "https://example.test/v99.0.0/request-eagle-update.json",
                        }],
                    }])
                    .to_string()
                } else if uri.contains("v99.0.0") {
                    manifest("99.0.0")
                } else {
                    manifest("1.2.3")
                };

                Ok(Response::builder().status(200).body(body.into()).unwrap())
            }
        }
    });

    cx.update(gpui_kit::init);
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await
        .unwrap();
    cx.update(|cx| cx.set_http_client(http));
    let updater = cx.update(|cx| updater::init("1.2.3", cx));
    let (_, view) =
        cx.add_window_view(|window, cx| GeneralSettings::new(updater.clone(), window, cx));
    let toggle = view.debug_bounds("daily-builds").unwrap();

    view.simulate_click(toggle.center(), Modifiers::default());
    view.run_until_parked();

    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["update_channel"], "daily");
    view.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Available(manifest) if manifest.version == "99.0.0"
        ));
    });

    // The available update adds a line above the switch.
    let toggle = view.debug_bounds("daily-builds").unwrap();
    view.simulate_click(toggle.center(), Modifiers::default());
    view.run_until_parked();

    view.read(|cx| {
        assert_eq!(
            cx.global::<preferences::Preferences>().update_channel,
            preferences::UpdateChannel::Stable
        );
        assert!(matches!(updater.read(cx).status(), UpdateStatus::UpToDate));
    });
    assert_eq!(requests.lock().unwrap().len(), 3);
}

#[gpui_kit::test]
fn switches_fit_general_at_each_zoom_and_theme(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let updater = cx.update(|cx| updater::init("1.2.3", cx));
    let (_, view) = cx.add_window_view(|window, cx| GeneralSettings::new(updater, window, cx));

    for theme in ["Default Light", "Default Dark"] {
        for font_size in [12., 16., 24.] {
            view.update(|window, cx| {
                assert!(request_eagle_theme::apply(theme, cx));
                gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size);
                window.set_rem_size(px(font_size));
                window.refresh();
            });
            view.simulate_resize(size(px(30. * font_size), px(40. * font_size)));

            for (row, toggle) in [
                ("daily-builds-row", "daily-builds"),
                ("vim-mode-row", "vim-mode"),
            ] {
                let row = view.debug_bounds(row).unwrap();
                let toggle = view.debug_bounds(toggle).unwrap();
                assert!(toggle.origin.x >= row.origin.x);
                assert!(toggle.right() <= row.right());
                assert!(toggle.bottom() <= row.bottom());
                assert!(toggle.bottom() <= px(40. * font_size));
            }
        }
    }
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
        preferences::init(cx);
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
        preferences::init(cx);
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
