use gpui_kit::component::select::SelectEvent;
use gpui_kit::{ClipboardItem, TestAppContext};
use preferences::{Preferences, ProxyMode, ProxyPreferences, ProxyProtocol};

use super::ProxySettings;

#[gpui_kit::test]
fn valid_edits_save_automatically_and_invalid_edits_keep_previous_settings(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        preferences::update(cx, |preferences| {
            preferences.request.proxy.mode = ProxyMode::Custom;
            preferences.request.proxy.host = "proxy.example.com".into();
        })
        .unwrap();
    });
    let (page, cx) = cx.add_window_view(ProxySettings::new);
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };

    cx.update(|window, cx| {
        page.read(cx)
            .port
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
        cx.write_to_clipboard(ClipboardItem::new_string("70000".into()));
    });
    cx.simulate_keystrokes(select_all);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(page.read(cx).error.is_some());
        assert_eq!(cx.global::<Preferences>().request.proxy.port, 8080);
    });

    cx.simulate_keystrokes(select_all);
    cx.simulate_input("3128");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(page.read(cx).error.is_none());
        let saved = &cx.global::<Preferences>().request.proxy;
        assert_eq!(saved.mode, ProxyMode::Custom);
        assert_eq!(saved.host, "proxy.example.com");
        assert_eq!(saved.port, 3128);
    });

    cx.simulate_keystrokes(select_all);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(page.read(cx).error.is_some());
        assert_eq!(cx.global::<Preferences>().request.proxy.port, 3128);
    });

    assert!(cx.debug_bounds("proxy-custom-fields").is_some());
    assert!(cx.debug_bounds("save-proxy").is_none());
}

#[gpui_kit::test]
fn an_invalid_custom_proxy_can_be_disabled(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        preferences::update(cx, |preferences| {
            preferences.request.proxy.mode = ProxyMode::Custom
        })
        .unwrap();
    });
    let (page, cx) = cx.add_window_view(ProxySettings::new);

    cx.update(|_, cx| {
        page.read(cx).mode.clone().update(cx, |_, cx| {
            cx.emit(SelectEvent::Confirm(Some("No proxy".into())));
        });
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(page.read(cx).error.is_none());
        assert_eq!(
            cx.global::<Preferences>().request.proxy.mode,
            ProxyMode::Disabled
        );
    });
}

#[gpui_kit::test]
async fn switching_modes_never_persists_rejected_host_credentials(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        preferences::load(directory.path(), cx)
    })
    .await
    .unwrap();
    cx.update(|cx| {
        preferences::update_proxy(
            ProxyPreferences {
                mode: ProxyMode::Custom,
                host: "original.example".into(),
                ..ProxyPreferences::default()
            },
            cx,
        )
    })
    .await
    .unwrap();
    let (page, cx) = cx.add_window_view(ProxySettings::new);

    cx.update(|window, cx| {
        page.read(cx)
            .host
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
        cx.write_to_clipboard(ClipboardItem::new_string(
            "synthetic-user:synthetic-password@proxy.example:3128".into(),
        ));
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-v"
    } else {
        "ctrl-a ctrl-v"
    });
    cx.run_until_parked();
    cx.read(|cx| assert!(page.read(cx).error.is_some()));

    for (label, mode) in [
        ("No proxy", ProxyMode::Disabled),
        ("Use system proxy", ProxyMode::System),
    ] {
        cx.update(|_, cx| {
            page.read(cx).mode.clone().update(cx, |_, cx| {
                cx.emit(SelectEvent::Confirm(Some(label.into())));
            });
        });
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(page.read(cx).error.is_none());
            let saved = &cx.global::<Preferences>().request.proxy;
            assert_eq!(saved.mode, mode);
            assert_eq!(saved.host, "original.example");
        });

        let document = std::fs::read_to_string(directory.path().join("preferences.json")).unwrap();
        assert!(!document.contains("synthetic-user"));
        assert!(!document.contains("synthetic-password"));
        let saved: Preferences = serde_json::from_str(&document).unwrap();
        assert_eq!(saved.request.proxy.mode, mode);
        assert_eq!(saved.request.proxy.host, "original.example");

        cx.update(|_, cx| {
            page.read(cx).mode.clone().update(cx, |_, cx| {
                cx.emit(SelectEvent::Confirm(Some("Use custom proxy".into())));
            });
        });
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(page.read(cx).error.is_some());
            assert_eq!(cx.global::<Preferences>().request.proxy.mode, mode);
        });
    }
}

#[gpui_kit::test]
fn pasting_proxy_urls_fills_and_saves_all_fields_together(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        preferences::update(cx, |preferences| {
            preferences.request.proxy.mode = ProxyMode::Custom;
            preferences.request.proxy.host = "original.example".into();
            preferences.request.proxy.http = false;
            preferences.request.proxy.bypass = "localhost".into();
        })
        .unwrap();
    });
    let (page, cx) = cx.add_window_view(ProxySettings::new);

    for (url, protocol, host, port, username, password) in [
        (
            "http://proxy-user:demo-password@proxy.example.com:20100",
            ProxyProtocol::Http,
            "proxy.example.com",
            "20100",
            "proxy-user",
            "demo-password",
        ),
        (
            "https://user%40team:p%3Ass%2Fword@proxy.example:8443",
            ProxyProtocol::Https,
            "proxy.example",
            "8443",
            "user@team",
            "p:ss/word",
        ),
        (
            "http://new-proxy.example",
            ProxyProtocol::Http,
            "new-proxy.example",
            "80",
            "",
            "",
        ),
    ] {
        cx.update(|window, cx| {
            page.read(cx)
                .host
                .clone()
                .update(cx, |input, cx| input.focus(window, cx));
            // Import must hide the next password even if the previous one was revealed.
            page.read(cx)
                .password
                .clone()
                .update(cx, |input, cx| input.set_masked(false, window, cx));
            cx.write_to_clipboard(ClipboardItem::new_string(url.into()));
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-v"
        } else {
            "ctrl-v"
        });
        cx.run_until_parked();

        cx.read(|cx| {
            let page = page.read(cx);
            assert_eq!(page.host.read(cx).value(), host);
            assert_eq!(page.port.read(cx).value(), port);
            assert_eq!(page.username.read(cx).value(), username);
            assert_eq!(page.password.read(cx).value(), password);
            assert!(page.password.read(cx).presentation().is_masked());
            assert_eq!(page.draft.protocol, protocol);
            assert_eq!(
                page.protocol.read(cx).selected_value().unwrap(),
                if protocol == ProxyProtocol::Http {
                    "http"
                } else {
                    "https"
                }
            );
            assert_eq!(page.draft.authentication, !username.is_empty());
            assert!(!page.draft.http);
            assert_eq!(page.draft.bypass, "localhost");
            let saved = &cx.global::<Preferences>().request.proxy;
            assert_eq!(saved.host, host);
            assert_eq!(saved.port.to_string(), port);
            assert_eq!(saved.protocol, protocol);
            assert_eq!(saved.username, username);
            assert_eq!(saved.password, password);
            assert_eq!(saved.authentication, !username.is_empty());
            assert!(!saved.http);
            assert_eq!(saved.bypass, "localhost");
        });
    }

    cx.read(|cx| {
        let saved = &cx.global::<Preferences>().request.proxy;
        assert_eq!(saved.host, "new-proxy.example");
        assert_eq!(saved.port, 80);
        assert!(!saved.authentication);
        assert!(saved.username.is_empty());
        assert!(saved.password.is_empty());
    });
}

#[gpui_kit::test]
fn invalid_url_pastes_leave_existing_fields_unchanged(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        preferences::update(cx, |preferences| {
            preferences.request.proxy.mode = ProxyMode::Custom;
            preferences.request.proxy.host = "original.example".into();
        })
        .unwrap();
    });
    let (page, cx) = cx.add_window_view(ProxySettings::new);

    cx.update(|window, cx| {
        page.read(cx)
            .host
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
        cx.write_to_clipboard(ClipboardItem::new_string(
            "socks5://user:demo-password@proxy.example:1080".into(),
        ));
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    });
    cx.run_until_parked();

    cx.read(|cx| {
        let page = page.read(cx);
        assert_eq!(page.host.read(cx).value(), "original.example");
        assert_eq!(page.port.read(cx).value(), "8080");
        assert!(!page.error.as_ref().unwrap().contains("demo-password"));
    });

    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("plain.example".into())));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-v"
    } else {
        "ctrl-a ctrl-v"
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(page.read(cx).host.read(cx).value(), "plain.example");
        assert!(page.read(cx).error.is_none());
    });
}
