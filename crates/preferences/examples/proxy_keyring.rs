//! Manual native-store smoke test. Uses a temporary preferences directory and
//! synthetic credentials, then removes its keyring entry. Never uses app settings.
use anyhow::{Result, ensure};
use gpui_kit::{App, AsyncApp};
use preferences::{Preferences, ProxyPreferences};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn main() {
    let passed = Arc::new(AtomicBool::new(false));
    let result = passed.clone();
    gpui_kit::application().run(move |cx: &mut App| {
        cx.spawn(async move |cx| {
            match round_trip(cx).await {
                Ok(()) => {
                    println!("Native proxy credential round trip and cleanup passed");
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
        username: "request-eagle-smoke-user".into(),
        password: "request-eagle-smoke-password".into(),
        ..ProxyPreferences::default()
    };
    cx.update(|cx| preferences::update_proxy(proxy.clone(), cx))
        .await?;
    let check: Result<()> = async {
        let document = fs::read_to_string(directory.path().join("preferences.json"))?;
        ensure!(!document.contains(&proxy.username) && !document.contains(&proxy.password));
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await?;
        ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy == proxy));
        Ok(())
    }
    .await;
    cx.update(|cx| preferences::update_proxy(ProxyPreferences::default(), cx))
        .await?;
    check
}
