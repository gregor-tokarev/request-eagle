//! Manual native-store smoke test. Uses a temporary preferences directory and
//! synthetic credentials, then removes its keyring entry. Never uses app settings.
use anyhow::{Result, ensure};
use gpui_kit::{App, AsyncApp};
use preferences::{Preferences, ProxyMode, ProxyPreferences};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn main() {
    let unavailable = std::env::args().any(|argument| argument == "--unavailable");
    let passed = Arc::new(AtomicBool::new(false));
    let result = passed.clone();
    gpui_kit::application().run(move |cx: &mut App| {
        cx.spawn(async move |cx| {
            let check = if unavailable {
                missing_provider(cx).await
            } else {
                round_trip(cx).await
            };

            match check {
                Ok(()) => {
                    println!("Native proxy credential test passed");
                    passed.store(true, Ordering::SeqCst);
                }
                Err(error) => eprintln!("Native proxy credential test failed: {error:#}"),
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    if !result.load(Ordering::SeqCst) {
        std::process::exit(1);
    }
}

async fn round_trip(cx: &mut AsyncApp) -> Result<()> {
    let directory = tempfile::tempdir()?;
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await?;
    let proxy = ProxyPreferences {
        mode: ProxyMode::Custom,
        host: "proxy.example.invalid".into(),
        authentication: true,
        username: "request-eagle-smoke-user".into(),
        password: "request-eagle-smoke-password".into(),
        ..ProxyPreferences::default()
    };
    cx.update(|cx| preferences::update_proxy(proxy.clone(), cx))
        .await?;
    let path = directory.path().join("preferences.json");
    let document = fs::read_to_string(&path)?;
    let check: Result<()> = async {
        ensure!(!document.contains(&proxy.username) && !document.contains(&proxy.password));
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await?;
        ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy == proxy));
        Ok(())
    }
    .await;
    cx.update(|cx| preferences::update_proxy(ProxyPreferences::default(), cx))
        .await?;
    check?;

    // Reload the old reference to verify deletion, not just the JSON update.
    fs::write(&path, document)?;
    ensure!(
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await
            .is_err()
    );
    ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy.validate().is_err()));
    Ok(())
}

/// Run only on an isolated session bus with no Secret Service provider.
async fn missing_provider(cx: &mut AsyncApp) -> Result<()> {
    let directory = tempfile::tempdir()?;
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await?;
    let proxy = ProxyPreferences {
        mode: ProxyMode::Custom,
        host: "proxy.example.invalid".into(),
        ..ProxyPreferences::default()
    };
    cx.update(|cx| preferences::update_proxy(proxy.clone(), cx))
        .await?;
    let path = directory.path().join("preferences.json");
    let original = fs::read(&path)?;
    let authenticated = ProxyPreferences {
        authentication: true,
        username: "request-eagle-smoke-user".into(),
        password: "request-eagle-smoke-password".into(),
        ..proxy.clone()
    };
    ensure!(
        cx.update(|cx| preferences::update_proxy(authenticated, cx))
            .await
            .is_err()
    );
    ensure!(fs::read(&path)? == original);
    ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy == proxy));

    // Simulate a saved credential whose provider was removed between launches.
    let mut document: serde_json::Value = serde_json::from_slice(&original)?;
    document["proxy_credentials_id"] = uuid::Uuid::new_v4().to_string().into();
    document["request"]["proxy"]["authentication"] = true.into();
    fs::write(&path, serde_json::to_vec(&document)?)?;
    ensure!(
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await
            .is_err()
    );
    ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy.validate().is_err()));
    let mut disabled = cx.read_global::<Preferences, _>(|p, _| p.request.proxy.clone());
    disabled.mode = ProxyMode::Disabled;
    cx.update(|cx| preferences::update_proxy(disabled, cx))
        .await?;
    ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy.validate().is_ok()));
    Ok(())
}
