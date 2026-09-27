use crate::workspace::Layout;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    path: &std::path::Path,
) -> (Entity<Layout>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    cx.add_window_view(|window, cx| {
        Layout::new(
            collection::CollectionRegistry::from_path(path).unwrap(),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    })
}

fn call(
    layout: &Entity<Layout>,
    cx: &mut VisualTestContext,
    command: Value,
) -> Result<Value, String> {
    let command = serde_json::from_value(command).unwrap();
    cx.update(|window, cx| {
        layout.update(cx, |layout, cx| {
            layout.automation_command(command, window, cx)
        })
    })
}

#[gpui_kit::test]
fn automation_edits_live_draft_saves_and_relocates_without_losing_changes(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let (layout, cx) = setup(cx, temp.path());
    let collection =
        call(&layout, cx, json!({"command":"collections.create"})).unwrap()["path"].clone();
    let tab = call(&layout, cx, json!({"command":"tabs.new"})).unwrap()["tab"].clone();
    let request = json!({"method":"POST","url":"https://example.test/first","body":"{\"a\":1}","headers":[["X-Test","1"]]});
    call(
        &layout,
        cx,
        json!({"command":"drafts.set","tab":tab,"request":request}),
    )
    .unwrap();
    assert!(call(&layout, cx, json!({"command":"tabs.close","tab":tab})).is_err());
    let saved = call(
        &layout,
        cx,
        json!({"command":"drafts.save","tab":tab,"parent":collection,"name":"First"}),
    )
    .unwrap();
    let path = saved["path"].clone();
    assert!(std::path::Path::new(path.as_str().unwrap()).exists());
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":tab})).unwrap()["dirty"],
        false
    );
    let edited = json!({"method":"GET","url":"https://example.test/unsaved"});
    call(
        &layout,
        cx,
        json!({"command":"drafts.set","tab":tab,"request":edited}),
    )
    .unwrap();
    call(&layout, cx, json!({"command":"requests.open","path":path})).unwrap();
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":tab})).unwrap()["request"]["url"],
        edited["url"]
    );
    let renamed = call(
        &layout,
        cx,
        json!({"command":"entries.rename","path":path,"name":"Renamed"}),
    )
    .unwrap()["path"]
        .clone();
    cx.run_until_parked();
    let tabs = call(&layout, cx, json!({"command":"tabs.list"})).unwrap();
    let open = tabs
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == tab)
        .unwrap();
    assert_eq!(open["path"], renamed);
    assert_eq!(open["dirty"], true);
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"entries.delete","path":renamed,"confirm":false})
        )
        .is_err()
    );
    call(&layout, cx, json!({"command":"drafts.save","tab":tab})).unwrap();
    assert_eq!(
        call(
            &layout,
            cx,
            json!({"command":"requests.get","path":renamed})
        )
        .unwrap()["request"]["url"],
        edited["url"]
    );
    call(&layout, cx, json!({"command":"tabs.close","tab":tab})).unwrap();
}

#[gpui_kit::test]
fn automation_script_trust_and_invalid_settings_leave_state_intact(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let (layout, cx) = setup(cx, temp.path());
    call(&layout, cx, json!({"command":"drafts.set","tab":1,"request":{"method":"GET","url":"http://127.0.0.1:1","pre_request":"console.log('test')"}})).unwrap();
    let error = call(&layout, cx, json!({"command":"requests.send","tab":1})).unwrap_err();
    assert!(error.contains("trust_scripts"));
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":1})).unwrap()["sending"],
        false
    );
    let before = call(&layout, cx, json!({"command":"settings.get"})).unwrap();
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"settings.appearance","interface_font_size":100})
        )
        .is_err()
    );
    assert_eq!(
        call(&layout, cx, json!({"command":"settings.get"})).unwrap(),
        before
    );
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"responses.get","tab":1,"limit":9999999})
        )
        .is_err()
    );
    assert!(call(&layout, cx, json!({"command":"drafts.get","tab":9999})).is_err());
}

#[test]
fn automation_proxy_endpoint_changes_cannot_reuse_redacted_credentials() {
    let saved = preferences::ProxyPreferences {
        mode: preferences::ProxyMode::Custom,
        protocol: preferences::ProxyProtocol::Https,
        host: "trusted.example".into(),
        port: 443,
        authentication: true,
        username: "stored-user".into(),
        password: "stored-secret".into(),
        ..Default::default()
    };
    let patch = |fields: Value, previous: preferences::ProxyPreferences| {
        let mut command = fields;
        command["command"] = json!("settings.proxy");
        super::settings::patched_proxy(serde_json::from_value(command).unwrap(), previous)
    };

    for endpoint in [
        json!({"host":"other.example"}),
        json!({"port":8080}),
        json!({"protocol":"http"}),
    ] {
        let changed = patch(endpoint.clone(), saved.clone()).unwrap();
        assert!(!changed.authentication);
        assert!(changed.username.is_empty());
        assert!(changed.password.is_empty());

        let mut authenticated = endpoint;
        authenticated["authentication"] = json!(true);
        assert!(patch(authenticated.clone(), saved.clone()).is_err());
        authenticated["username"] = json!("replacement-user");
        assert!(patch(authenticated.clone(), saved.clone()).is_err());
        authenticated["password"] = json!("replacement-secret");
        let changed = patch(authenticated, saved.clone()).unwrap();
        assert!(changed.authentication);
        assert_eq!(changed.username, "replacement-user");
        assert_eq!(changed.password, "replacement-secret");
    }

    let unchanged = patch(json!({"bypass":"localhost"}), saved.clone()).unwrap();
    assert_eq!(unchanged.username, saved.username);
    assert_eq!(unchanged.password, saved.password);

    let unavailable = preferences::ProxyPreferences {
        credentials_unavailable: true,
        username: String::new(),
        password: String::new(),
        ..saved
    };
    assert!(patch(json!({"host":"other.example"}), unavailable).is_err());
}

#[test]
fn automation_can_retry_and_replace_unavailable_proxy_credentials() {
    let unavailable = preferences::ProxyPreferences {
        mode: preferences::ProxyMode::Custom,
        host: "trusted.example".into(),
        authentication: true,
        credentials_unavailable: true,
        ..Default::default()
    };
    let patch = |fields: Value| {
        let mut command = fields;
        command["command"] = json!("settings.proxy");
        super::settings::patched_proxy(
            serde_json::from_value(command).unwrap(),
            unavailable.clone(),
        )
    };

    let retry = patch(json!({})).unwrap();
    assert!(
        retry.credentials_unavailable,
        "The store must still retry the keyring"
    );
    let replacement = patch(json!({"username":"new-user","password":"new-secret"})).unwrap();
    assert_eq!(replacement.password, "new-secret");

    assert!(patch(json!({"host":"other.example","username":"","password":""})).is_err());
    let replacement = patch(json!({"host":"other.example","username":"new-user","password":"new-secret","authentication":true})).unwrap();
    assert!(
        !replacement.credentials_unavailable,
        "Never restore the previous endpoint's secret over explicit replacements"
    );
    assert!(replacement.authentication);
    assert_eq!(replacement.username, "new-user");
    assert_eq!(replacement.password, "new-secret");
}

#[gpui_kit::test]
fn automation_rejects_non_utf8_collection_and_tab_paths(cx: &mut TestAppContext) {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let temp = tempfile::tempdir().unwrap();
    let collection = temp.path().join(OsString::from_vec(vec![b'a', 0xff]));
    std::fs::create_dir(&collection).unwrap();
    let (layout, cx) = setup(cx, temp.path());
    let error = call(&layout, cx, json!({"command":"collections.list"})).unwrap_err();
    assert!(error.contains("UTF-8"));

    cx.update(|_, cx| {
        layout.read(cx).main_view.clone().update(cx, |view, cx| {
            view.open_request(
                &collection.join("request.toml"),
                "id".into(),
                "Request".into(),
                "Collection".into(),
                Vec::new(),
                &collection::HttpRequest::default().into(),
                cx,
            );
        });
    });
    assert!(
        call(&layout, cx, json!({"command":"tabs.list"}))
            .unwrap_err()
            .contains("UTF-8")
    );
    assert!(
        call(&layout, cx, json!({"command":"drafts.save","tab":2}))
            .unwrap_err()
            .contains("UTF-8")
    );
    assert!(call(&layout, cx, json!({"command":"app.status"})).is_ok());
}

#[gpui_kit::test]
fn automation_checks_collection_root_encoding_before_creating_files(cx: &mut TestAppContext) {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(OsString::from_vec(vec![b'a', 0xff]));
    std::fs::create_dir(&root).unwrap();
    let (layout, cx) = setup(cx, &root);
    assert!(
        call(&layout, cx, json!({"command":"collections.create"}))
            .unwrap_err()
            .contains("UTF-8")
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[gpui_kit::test]
fn automation_rejects_unsupported_app_updates_without_network_or_state_changes(
    cx: &mut TestAppContext,
) {
    let temp = tempfile::tempdir().unwrap();
    let (layout, cx) = setup(cx, temp.path());
    cx.update(|_, cx| {
        cx.set_http_client(gpui_kit::http_client::FakeHttpClient::create(|_| async {
            panic!("Unsupported app updates must not make network requests")
        }))
    });
    let status = call(&layout, cx, json!({"command":"updates.status"})).unwrap();
    assert_eq!(status["supported"], false);
    assert_eq!(status["state"], "unsupported");
    for command in ["updates.check", "updates.download", "updates.install"] {
        let input = if command == "updates.install" {
            json!({"command":command,"confirm":true})
        } else {
            json!({"command":command})
        };
        assert!(
            call(&layout, cx, input)
                .unwrap_err()
                .contains("only supported")
        );
    }
    cx.read(|cx| {
        assert!(matches!(
            layout.read(cx).updater.read(cx).status(),
            updater::UpdateStatus::Idle
        ))
    });
}
