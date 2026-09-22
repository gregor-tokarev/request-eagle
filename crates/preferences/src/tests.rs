use crate::credentials::CredentialStore;
use crate::{
    AppearanceMode, Preferences, ProxyMode, ProxyPreferences, init, load, update, update_proxy,
};
use anyhow::Result;
use gpui_kit::{App, Task, TestAppContext};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fs,
    rc::Rc,
};

#[derive(Default)]
struct MemoryCredentials {
    entries: RefCell<HashMap<String, Vec<u8>>>,
    fail_read: Cell<bool>,
    fail_write: Cell<bool>,
    write_gate: RefCell<Option<futures_channel::oneshot::Receiver<()>>>,
}

impl CredentialStore for MemoryCredentials {
    fn read(&self, id: &str, _: &App) -> Task<Result<Option<Vec<u8>>>> {
        Task::ready(if self.fail_read.get() {
            Err(anyhow::anyhow!("Keyring locked"))
        } else {
            Ok(self.entries.borrow().get(id).cloned())
        })
    }

    fn write(&self, id: &str, secret: &[u8], cx: &App) -> Task<Result<()>> {
        if self.fail_write.get() {
            return Task::ready(Err(anyhow::anyhow!("Keyring unavailable")));
        }
        self.entries.borrow_mut().insert(id.into(), secret.into());
        let gate = self.write_gate.borrow_mut().take();
        cx.background_executor().spawn(async move {
            if let Some(gate) = gate {
                gate.await?;
            }
            Ok(())
        })
    }

    fn delete(&self, id: &str, _: &App) -> Task<Result<()>> {
        self.entries.borrow_mut().remove(id);
        Task::ready(Ok(()))
    }
}

fn install_store(cx: &mut TestAppContext) -> Rc<MemoryCredentials> {
    let store = Rc::new(MemoryCredentials::default());
    cx.update(|cx| crate::store::set_credential_store(store.clone(), cx));
    store
}

fn proxy() -> ProxyPreferences {
    ProxyPreferences {
        mode: ProxyMode::Custom,
        host: "proxy.example.com".into(),
        port: 3128,
        authentication: true,
        username: "proxy-user".into(),
        password: "proxy-password".into(),
        bypass: "localhost, 127.0.0.1".into(),
        ..ProxyPreferences::default()
    }
}

fn assert_no_credentials(path: &std::path::Path) {
    let bytes = fs::read(path).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(document["request"]["proxy"].get("username").is_none());
    assert!(document["request"]["proxy"].get("password").is_none());
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("proxy-user"));
    assert!(!text.contains("proxy-password"));
}

#[gpui_kit::test]
async fn proxy_credentials_survive_reload_without_entering_json(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    assert_no_credentials(&directory.path().join("preferences.json"));
    assert_eq!(store.entries.borrow().len(), 1);
    let secret: serde_json::Value =
        serde_json::from_slice(store.entries.borrow().values().next().unwrap()).unwrap();
    assert_eq!(secret["username"], "proxy-user");
    assert_eq!(secret["password"], "proxy-password");
    cx.update(|cx| cx.set_global(Preferences::default()));
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, proxy()));
}

#[gpui_kit::test]
async fn legacy_credentials_are_migrated_before_json_is_replaced(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    fs::write(&path, r#"{"appearance":{"editor_font":"Menlo"},"request":{"proxy":{"mode":"custom","host":"proxy.example.com","authentication":true,"username":"proxy-user","password":"proxy-password"}}}"#).unwrap();
    let original = fs::read(&path).unwrap();
    store.fail_write.set(true);
    assert!(cx.update(|cx| load(directory.path(), cx)).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    cx.update(|cx| {
        assert!(cx.global::<Preferences>().request.proxy.validate().is_err());
        assert_eq!(cx.global::<Preferences>().appearance.editor_font, "Menlo");
        assert!(update(cx, |p| p.appearance.mode = AppearanceMode::Light).is_err());
    });

    store.fail_write.set(false);
    cx.update(|cx| update_proxy(cx.global::<Preferences>().request.proxy.clone(), cx))
        .await
        .unwrap();
    assert_no_credentials(&path);
    assert_eq!(store.entries.borrow().len(), 1);
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| {
        assert_eq!(
            cx.global::<Preferences>().request.proxy.password,
            "proxy-password"
        );
        assert!(crate::credential_error(cx).is_none());
    });
}

#[gpui_kit::test]
async fn failed_keyring_write_keeps_previous_file_credentials_and_runtime(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    let path = directory.path().join("preferences.json");
    let original = fs::read(&path).unwrap();
    store.fail_write.set(true);
    let mut replacement = proxy();
    replacement.password = "replacement".into();
    assert!(cx.update(|cx| update_proxy(replacement, cx)).await.is_err());
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(store.entries.borrow().len(), 1);
    cx.read(|cx| {
        assert_eq!(cx.global::<Preferences>().request.proxy, proxy());
        assert!(crate::credential_error(cx).is_some());
    });
    store.fail_write.set(false);
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    cx.read(|cx| assert!(crate::credential_error(cx).is_none()));
}

#[gpui_kit::test]
async fn failed_file_save_removes_staged_secret_and_keeps_original(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    let before = store.entries.borrow().clone();
    let path = directory.path().join("preferences.json");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let mut replacement = proxy();
    replacement.password = "replacement".into();
    assert!(cx.update(|cx| update_proxy(replacement, cx)).await.is_err());
    assert_eq!(*store.entries.borrow(), before);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, proxy()));
}

#[gpui_kit::test]
async fn clearing_credentials_deletes_the_entry_and_reference(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    let mut cleared = proxy();
    cleared.username.clear();
    cleared.password.clear();
    cleared.authentication = false;
    cx.update(|cx| update_proxy(cleared.clone(), cx))
        .await
        .unwrap();
    assert!(store.entries.borrow().is_empty());
    assert_no_credentials(&directory.path().join("preferences.json"));
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| {
        assert_eq!(cx.global::<Preferences>().request.proxy, cleared);
        assert!(cx.global::<Preferences>().proxy_credentials_id.is_none());
    });
}

#[gpui_kit::test]
async fn locked_keyring_blocks_authenticated_requests_and_can_be_retried(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    store.fail_read.set(true);
    assert!(cx.update(|cx| load(directory.path(), cx)).await.is_err());
    let loaded = cx.read(|cx| cx.global::<Preferences>().request.proxy.clone());
    assert!(loaded.validate().is_err());
    assert!(
        request::RequestExecutor::new(&crate::RequestPreferences {
            proxy: loaded.clone(),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        cx.update(|cx| update_proxy(loaded.clone(), cx))
            .await
            .is_err()
    );
    assert_no_credentials(&directory.path().join("preferences.json"));

    let mut disabled = loaded;
    disabled.mode = ProxyMode::Disabled;
    cx.update(|cx| update_proxy(disabled, cx)).await.unwrap();
    store.fail_read.set(false);
    let mut retry = cx.read(|cx| cx.global::<Preferences>().request.proxy.clone());
    retry.mode = ProxyMode::Custom;
    let first = cx.update(|cx| update_proxy(retry.clone(), cx));
    let second = cx.update(|cx| update_proxy(retry, cx));
    first.await.unwrap();
    second.await.unwrap();
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, proxy()));
}

#[gpui_kit::test]
async fn missing_credentials_require_reentry_and_do_not_fall_back(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    store.entries.borrow_mut().clear();
    assert!(cx.update(|cx| load(directory.path(), cx)).await.is_err());
    cx.read(|cx| assert!(cx.global::<Preferences>().request.proxy.validate().is_err()));
    cx.update(|cx| update_proxy(proxy(), cx)).await.unwrap();
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, proxy()));
}

#[gpui_kit::test]
async fn queued_proxy_edits_preserve_other_settings_and_finish_in_order(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    let (release, gate) = futures_channel::oneshot::channel();
    *store.write_gate.borrow_mut() = Some(gate);
    let first = cx.update(|cx| update_proxy(proxy(), cx));
    cx.run_until_parked();
    assert_eq!(store.entries.borrow().len(), 1);
    let mut last_proxy = proxy();
    last_proxy.password = "latest-password".into();
    let second = cx.update(|cx| update_proxy(last_proxy.clone(), cx));
    cx.update(|cx| update(cx, |p| p.appearance.editor_font = "Menlo".into()))
        .unwrap();
    release.send(()).unwrap();
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(store.entries.borrow().len(), 1);
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| {
        assert_eq!(cx.global::<Preferences>().request.proxy, last_proxy);
        assert_eq!(cx.global::<Preferences>().appearance.editor_font, "Menlo");
    });
}

#[gpui_kit::test]
async fn partial_documents_default_missing_fields_and_save_normally(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    fs::write(
        &path,
        r#"{"appearance":{"interface_font_size":99,"editor_font":"  Menlo  "}}"#,
    )
    .unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| {
        let appearance = &cx.global::<Preferences>().appearance;
        assert_eq!(appearance.interface_font_size, 99.);
        assert_eq!(appearance.editor_font, "  Menlo  ");
        assert_eq!(appearance.mode, AppearanceMode::Dark);
        update(cx, |p| p.appearance.interface_font_size = 30.).unwrap();
    });
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| {
        assert_eq!(
            cx.global::<Preferences>().appearance.interface_font_size,
            30.
        )
    });
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[gpui_kit::test]
async fn malformed_file_is_not_overwritten_and_can_be_reloaded_after_repair(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    fs::write(&path, "invalid json").unwrap();
    cx.update(|cx| {
        init(cx);
        update(cx, |p| p.appearance.interface_font_size = 18.).unwrap();
    });
    assert!(cx.update(|cx| load(directory.path(), cx)).await.is_err());
    cx.update(|cx| {
        assert!(update(cx, |p| p.appearance.mode = AppearanceMode::Light).is_err());
        assert_eq!(
            cx.global::<Preferences>().appearance.interface_font_size,
            18.
        );
    });
    assert!(cx.update(|cx| update_proxy(proxy(), cx)).await.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid json");
    fs::write(&path, "{}").unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| update(cx, |p| p.appearance.interface_font_size = 19.))
        .unwrap();
}

#[gpui_kit::test]
fn updates_can_run_without_a_storage_directory(cx: &mut TestAppContext) {
    cx.update(|cx| {
        update(cx, |p| p.appearance.editor_font = "Menlo".into()).unwrap();
        init(cx);
        assert_eq!(cx.global::<Preferences>().appearance.editor_font, "Menlo");
    });
}

#[test]
fn serialization_never_includes_credentials_but_accepts_legacy_values() {
    let serialized = serde_json::to_value(proxy()).unwrap();
    assert!(serialized.get("username").is_none());
    assert!(serialized.get("password").is_none());
    let legacy: ProxyPreferences =
        serde_json::from_str(r#"{"username":"proxy-user","password":"proxy-password"}"#).unwrap();
    assert_eq!(legacy.username, "proxy-user");
    assert_eq!(legacy.password, "proxy-password");
}

#[gpui_kit::test]
async fn saves_and_reloads_the_shared_document(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| {
        assert_eq!(cx.global::<Preferences>(), &Preferences::default());
        assert!(!path.exists());
        update(cx, |preferences| {
            preferences.appearance.mode = AppearanceMode::System;
            preferences.appearance.dark_theme = "Catppuccin Mocha".into();
            preferences.appearance.editor_font = "Menlo".into();
            preferences.request.follow_all_redirects = false;
        })
        .unwrap();
        update(cx, |preferences| {
            preferences.appearance.interface_font_size = 20.
        })
        .unwrap();
    });
    let expected = cx.read(|cx| cx.global::<Preferences>().clone());
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["appearance"]["editor_font"], "Menlo");
    assert_eq!(document["appearance"]["interface_font_size"], 20.);
    assert_eq!(document["request"]["follow_all_redirects"], false);
    cx.update(|cx| cx.set_global(Preferences::default()));
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| assert_eq!(cx.global::<Preferences>(), &expected));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[gpui_kit::test]
async fn failed_ordinary_save_does_not_publish_changes_or_leave_temporary_files(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.update(|cx| {
        update(cx, |p| p.appearance.editor_font = "Menlo".into()).unwrap();
        let original = cx.global::<Preferences>().clone();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(update(cx, |p| p.appearance.mode = AppearanceMode::Light).is_err());
        assert_eq!(cx.global::<Preferences>(), &original);
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    });
}

#[gpui_kit::test]
async fn closing_the_editor_keeps_the_latest_queued_save(cx: &mut TestAppContext) {
    let store = install_store(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    let mut latest = proxy();
    latest.password = "latest-password".into();
    cx.update(|cx| {
        drop(update_proxy(proxy(), cx));
        drop(update_proxy(latest.clone(), cx));
    });
    cx.run_until_parked();
    assert_eq!(store.entries.borrow().len(), 1);
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, latest));
    cx.update(|cx| load(directory.path(), cx)).await.unwrap();
    cx.read(|cx| assert_eq!(cx.global::<Preferences>().request.proxy, latest));
}
