use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;

use super::VariableStore;

struct DialogHost;

impl gpui_kit::Render for DialogHost {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement as _, Styled as _};
        gpui_kit::div()
            .size_full()
            .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
    }
}

#[gpui_kit::test]
fn selecting_an_existing_secret_preserves_its_value_in_a_masked_editor(cx: &mut TestAppContext) {
    use gpui_kit::{
        AppContext as _, Modifiers,
        component::{Root, WindowExt as _},
    };

    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        VariableStore::global(cx).update(cx, |store, _| {
            store.environments.insert(None, Default::default());
            store
                .secrets
                .insert("token".into(), "keep-existing-value".into());
        });
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| DialogHost);
        Root::new(host, window, cx)
    });
    cx.update(|window, cx| {
        let scope = cx.new(|_| super::VariableScope { path: None });
        super::open_manager(scope, window, cx);
    });
    for selector in ["variable-source-Secrets", "variable-manager-entry-0"] {
        cx.run_until_parked();
        // Mount the dialog before measuring its reduced-motion position.
        for _ in 0..2 {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        }
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }
    cx.update(|window, cx| {
        let focused = window.focused_input(cx).unwrap();
        let input = focused.as_input().unwrap().read(cx);
        assert_eq!(input.value(), "keep-existing-value");
        assert!(input.presentation().is_masked());
    });
}

#[gpui_kit::test]
fn variable_manager_waits_for_saves_before_done_or_escape(cx: &mut TestAppContext) {
    use gpui_kit::{AppContext as _, Modifiers, component::Root};

    let store = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        let store = VariableStore::global(cx);
        store.update(cx, |store, _| store.saving = true);
        store
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| DialogHost);
        Root::new(host, window, cx)
    });
    cx.update(|window, cx| {
        let scope = cx.new(|_| super::VariableScope { path: None });
        super::open_manager(scope, window, cx);
    });
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    let done = cx.debug_bounds("close-variables").unwrap();
    cx.simulate_click(done.center(), Modifiers::default());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("variable-manager").is_some());

    cx.update(|_, cx| {
        store.update(cx, |store, cx| {
            store.saving = false;
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    let done = cx.debug_bounds("close-variables").unwrap();
    cx.simulate_click(done.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("variable-manager").is_none());
}

#[gpui_kit::test]
async fn environment_saves_merge_external_changes_and_reject_invalid_files(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    std::fs::write(&path, "existing = 'cached'\nremoved = 'cached'").unwrap();
    let store = cx.update(VariableStore::global);
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.ensure_environment(&Some(path.clone()), cx)
        });
    });

    for value in [Some("local value".to_owned()), None] {
        let source =
            "existing = 'external update'\nexternal = 'added externally'\nlocal = 'previous'";
        std::fs::write(&path, source).unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| {
                store.save_entry(Some(path.clone()), false, "local".into(), value.clone(), cx)
            });
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while cx.read(|cx| store.read(cx).saving) {
            assert!(Instant::now() < deadline);
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }
        let saved = environment::Environment::from_file(&path).unwrap();
        assert_eq!(saved.resolve("existing"), Some("external update"));
        assert_eq!(saved.resolve("external"), Some("added externally"));
        assert_eq!(saved.resolve("removed"), None);
        assert_eq!(saved.resolve("local"), value.as_deref());
        cx.read(|cx| {
            assert_eq!(
                store
                    .read(cx)
                    .values(&Some(path.clone()))
                    .unwrap()
                    .environment,
                saved.entries
            );
        });
    }

    std::fs::write(&path, "invalid = [").unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.save_entry(
                Some(path.clone()),
                false,
                "local".into(),
                Some("discard".into()),
                cx,
            )
        });
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while cx.read(|cx| store.read(cx).saving) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid = [");
    cx.read(|cx| {
        assert!(store.read(cx).save_error.is_some());
        assert!(
            !store
                .read(cx)
                .values(&Some(path))
                .unwrap()
                .environment
                .contains_key("local")
        );
    });
}

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
