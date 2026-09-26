use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;

use super::VariableStore;

#[gpui_kit::test]
async fn environment_edits_persist_and_failed_saves_preserve_existing_values(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    let store = cx.update(VariableStore::global);
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.ensure_environment(&Some(path.clone()), cx);
            store.save_entry(
                Some(path.clone()),
                false,
                "base_url".into(),
                Some("https://example.com".into()),
                cx,
            );
        })
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while cx.read(|cx| store.read(cx).saving) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("base_url"),
        Some("https://example.com")
    );

    // Replacing a directory with a file fails even when tests run as root.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.save_entry(
                Some(path.clone()),
                false,
                "base_url".into(),
                Some("should-not-commit".into()),
                cx,
            )
        })
    });
    while cx.read(|cx| store.read(cx).saving) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    cx.read(|cx| {
        assert!(store.read(cx).save_error.is_some());
        assert_eq!(
            store.read(cx).values(&Some(path)).unwrap().environment["base_url"],
            "https://example.com"
        );
    });
}
