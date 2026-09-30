use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use gpui_kit::{
    TestAppContext,
    http_client::{FakeHttpClient, Response},
};

use crate::actions::CheckForUpdates;
use updater::UpdateStatus;

#[gpui_kit::test]
fn menu_starts_check_and_opens_general(cx: &mut TestAppContext) {
    let requests = Arc::new(AtomicUsize::new(0));
    let http = FakeHttpClient::create({
        let requests = requests.clone();
        move |_| {
            requests.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(Response::builder()
                    .status(200)
                    .body(r#"{"version":"99.0.0","url":"https://example.test/RequestEagle.zip","sha256":"abc123"}"#.into())
                    .unwrap())
            }
        }
    });

    let opened = Arc::new(AtomicUsize::new(0));
    let updater = cx.update(|cx| updater::init("1.2.3", cx));

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_http_client(http);
        crate::actions::init(updater.clone(), cx);

        let opened = opened.clone();
        cx.on_action(move |_: &workspace::OpenGeneralSettings, _| {
            opened.fetch_add(1, Ordering::SeqCst);
        });
    });

    let (_, view) = cx.add_window_view(|_, _| gpui_kit::Empty);
    view.update(|window, _| window.activate_window());

    view.dispatch_action(CheckForUpdates);

    assert_eq!(opened.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    view.read(|cx| {
        assert!(matches!(
            updater.read(cx).status(),
            UpdateStatus::Available(_)
        ))
    });
}
