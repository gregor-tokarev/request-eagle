use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;

use super::VariableStore;

#[test]
fn concurrent_environment_processes_keep_each_edit() {
    use std::{fs::OpenOptions, process::Command};

    if let Ok(path) = std::env::var("REQUEST_EAGLE_ENVIRONMENT_WORKER_PATH") {
        let name = std::env::var("REQUEST_EAGLE_ENVIRONMENT_WORKER_NAME").unwrap();
        let path = std::path::Path::new(&path);
        std::fs::write(path.with_extension(&name), "ready").unwrap();
        super::environment_file::save_entry(path, None, &name, Some("saved")).unwrap();
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    std::fs::write(&path, "external = 'initial'\n").unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.path().join(".environment.toml.lock"))
        .unwrap();
    lock.lock().unwrap();
    let mut children: Vec<_> = (0..6)
        .map(|index| {
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "variables::tests::concurrent_environment_processes_keep_each_edit",
                ])
                .env("REQUEST_EAGLE_ENVIRONMENT_WORKER_PATH", &path)
                .env(
                    "REQUEST_EAGLE_ENVIRONMENT_WORKER_NAME",
                    format!("worker{index}"),
                )
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    for index in 0..6 {
        while !path.with_extension(format!("worker{index}")).exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert!(
        children
            .iter_mut()
            .all(|child| child.try_wait().unwrap().is_none())
    );
    std::fs::write(&path, "external = 'updated under lock'\n").unwrap();
    drop(lock);
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let entries = environment::Environment::from_file(&path).unwrap().entries;
    assert_eq!(entries["external"], "updated under lock");
    for index in 0..6 {
        assert_eq!(entries[&format!("worker{index}")], "saved");
    }
}

#[test]
fn renaming_environment_variables_removes_the_old_name_and_rejects_collisions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    std::fs::write(&path, "old = 'value'\nother = 'keep'").unwrap();
    let entries = super::environment_file::save_entry(&path, Some("old"), "new", None).unwrap();
    assert!(!entries.contains_key("old"));
    assert_eq!(entries["new"], "value");
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(
        super::environment_file::save_entry(&path, Some("new"), "other", Some("value")).is_err()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    assert!(super::environment_file::save_entry(&path, Some("removed"), "restored", None).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
}

#[cfg(unix)]
#[gpui_kit::test]
async fn shared_environment_aliases_refresh_after_save_and_reload(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("shared.toml");
    let first = directory.path().join("first.toml");
    let second = directory.path().join("second.toml");
    std::fs::write(&target, "base = 'old'").unwrap();
    for path in [&first, &second] {
        std::os::unix::fs::symlink(&target, path).unwrap();
    }
    let store = cx.update(VariableStore::global);
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            for path in [&first, &second] {
                store.ensure_environment(&Some(path.clone()), cx);
            }
            store.save_entry(
                Some(first.clone()),
                false,
                Some("base".into()),
                "renamed".into(),
                Some("new".into()),
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
    cx.read(|cx| {
        for path in [&first, &second] {
            let values = store.read(cx).values(&Some(path.clone())).unwrap();
            assert_eq!(values.environment["renamed"], "new");
            assert!(!values.environment.contains_key("base"));
        }
    });
    std::fs::write(&target, "external = 'fresh'").unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.reload_environment(&Some(second.clone()), cx);
            for path in [&first, &second] {
                assert_eq!(
                    store.values(&Some(path.clone())).unwrap().environment["external"],
                    "fresh"
                );
            }
        })
    });
}

#[cfg(unix)]
#[test]
fn environment_saves_update_symlink_targets_and_preserve_links() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("shared.toml");
    let link = directory.path().join("environment.toml");
    std::fs::write(&target, "# shared\nbase = 'old'\nkeep = 'value'\n").unwrap();
    symlink("shared.toml", &link).unwrap();

    super::environment_file::save_entry(&link, None, "base", Some("updated")).unwrap();
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        std::path::Path::new("shared.toml")
    );
    assert_eq!(
        environment::Environment::from_file(&target)
            .unwrap()
            .resolve("base"),
        Some("updated")
    );
    super::environment_file::save_entry(&link, None, "keep", None).unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        environment::Environment::from_file(&target)
            .unwrap()
            .resolve("keep"),
        None
    );

    std::fs::remove_file(&target).unwrap();
    assert!(
        super::environment_file::save_entry(&link, None, "base", Some("must not replace the link"))
            .is_err()
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn environment_edits_preserve_comments_order_and_unrelated_formatting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    let original = "# Shared config\n\"base.url\"  =  'old' # server\n\n# Keep this note\nuntouched = '''literal value'''\nremove = 'temporary'\n# End of file\n";
    std::fs::write(&path, original).unwrap();

    super::environment_file::save_entry(&path, None, "base.url", Some("old")).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let entries =
        super::environment_file::save_entry(&path, None, "base.url", Some("new")).unwrap();
    assert_eq!(entries["base.url"], "new");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original.replace("'old'", "\"new\"")
    );

    super::environment_file::save_entry(&path, None, "remove", None).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original
            .replace("'old'", "\"new\"")
            .replace("remove = 'temporary'\n", "")
    );
}

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
async fn unchanged_editors_preserve_external_updates_after_selection_and_save(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{
        AppContext as _, Modifiers, VisualTestContext,
        component::{Root, WindowExt as _},
    };

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    std::fs::write(&path, "value = 'original'").unwrap();
    let store = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        let store = VariableStore::global(cx);
        store.update(cx, |store, cx| {
            store.ensure_environment(&Some(path.clone()), cx)
        });
        store
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| DialogHost);
        Root::new(host, window, cx)
    });
    cx.update(|window, cx| {
        let scope = cx.new(|_| super::VariableScope {
            path: Some(path.clone()),
        });
        super::open_manager(scope, window, cx);
    });
    let click = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        }
        let bounds = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
    };
    click(cx, "variable-manager-entry-0");
    std::fs::write(&path, "value = 'external after selection'").unwrap();
    click(cx, "save-variable");
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("value"),
        Some("external after selection")
    );

    click(cx, "variable-manager-entry-0");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("edited and saved");
    click(cx, "save-variable");
    let deadline = Instant::now() + Duration::from_secs(5);
    while cx.read(|cx| store.read(cx).saving) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("value"),
        Some("edited and saved")
    );
    std::fs::write(&path, "value = 'external after save'").unwrap();
    click(cx, "save-variable");
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("value"),
        Some("external after save")
    );

    click(cx, "variable-manager-entry-0");
    std::fs::write(&path, "value = 'external before rename'").unwrap();
    click(cx, "variable-name-field");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("renamed");
    click(cx, "save-variable");
    let deadline = Instant::now() + Duration::from_secs(5);
    while cx.read(|cx| store.read(cx).saving) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    let saved = environment::Environment::from_file(&path).unwrap();
    assert_eq!(saved.resolve("value"), None);
    assert_eq!(saved.resolve("renamed"), Some("external before rename"));
    click(cx, "variable-value-field");
    cx.update(|window, cx| {
        let focused = window.focused_input(cx).unwrap();
        assert_eq!(
            focused.as_input().unwrap().read(cx).value(),
            "external before rename"
        );
    });
    std::fs::write(&path, "renamed = 'external after rename'").unwrap();
    click(cx, "save-variable");
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("renamed"),
        Some("external after rename")
    );
}

#[gpui_kit::test]
fn selecting_a_secret_clears_stale_errors_and_preserves_its_masked_value(cx: &mut TestAppContext) {
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
            store.save_error = Some("Failure from another collection".into());
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
        let store = VariableStore::global(cx);
        assert!(store.read(cx).save_error.is_none());
        store.update(cx, |store, _| {
            store.save_error = Some("Failure from the environment editor".into());
        });
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
        cx.update(|_, cx| {
            assert!(VariableStore::global(cx).read(cx).save_error.is_none());
        });
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
                store.save_entry(
                    Some(path.clone()),
                    false,
                    None,
                    "local".into(),
                    value.clone(),
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
                None,
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
            store
                .read(cx)
                .environment_errors
                .contains_key(&Some(path.clone()))
        );
        assert!(
            !store
                .read(cx)
                .values(&Some(path))
                .unwrap()
                .environment
                .contains_key("existing")
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
                None,
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

    let permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut read_only = permissions.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&path, read_only).unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.save_entry(
                Some(path.clone()),
                false,
                None,
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
            store
                .read(cx)
                .values(&Some(path.clone()))
                .unwrap()
                .environment["base_url"],
            "https://example.com"
        );
    });
    std::fs::set_permissions(&path, permissions).unwrap();
    std::fs::write(&path, "base_url = 'https://fixed.example'").unwrap();
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.reload_environment(&Some(path.clone()), cx);
            assert!(store.save_error.is_none());
            assert_eq!(
                store.values(&Some(path)).unwrap().environment["base_url"],
                "https://fixed.example"
            );
        });
    });
}
