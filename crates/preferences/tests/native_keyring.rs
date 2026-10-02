//! Native-store integration test. Uses a temporary preferences directory and
//! synthetic credentials, then removes its keyring entry. Never uses app settings.
use anyhow::{Result, ensure};
use gpui_kit::{App, AsyncApp};
use preferences::{CertificateFiles, ClientCertificate, Preferences, ProxyMode, ProxyPreferences};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn main() {
    let unavailable = std::env::args().any(|argument| argument == "--unavailable");

    // Ordinary cargo test runs must not open the user's native credential store.
    if !unavailable && !std::env::args().any(|argument| argument == "--round-trip") {
        println!("Native keyring checks skipped; run scripts/check-linux-keyring.sh on Linux.");
        return;
    }

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
    println!("Saving synthetic credentials to the native store");
    cx.update(|cx| preferences::update_proxy(proxy.clone(), cx))
        .await?;
    let path = directory.path().join("preferences.json");
    let document = fs::read_to_string(&path)?;
    let check: Result<()> = async {
        ensure!(!document.contains(&proxy.username) && !document.contains(&proxy.password));
        println!("Reloading credentials from the native store");
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await?;
        ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy == proxy));

        let file = preferences::PreferencesFile::new(directory.path());
        ensure!(file.request_preferences().await?.proxy == proxy);
        ensure!(file.read()?.request.proxy.password.is_empty());
        file.update_proxy(
            |_| Ok(()),
            Some(("headless-user".into(), "headless-password".into())),
        )
        .await?;
        let updated = fs::read_to_string(&path)?;
        ensure!(!updated.contains("headless-user") && !updated.contains("headless-password"));
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await?;
        ensure!(cx.read_global::<Preferences, _>(
                |p, _| p.request.proxy.password == "headless-password"
            ));
        Ok(())
    }
    .await;
    println!("Deleting synthetic credentials from the native store");
    cx.update(|cx| preferences::update_proxy(ProxyPreferences::default(), cx))
        .await?;
    check?;

    // Reload the old reference to verify deletion, not just the JSON update.
    println!("Verifying the deleted credential is unavailable");
    fs::write(&path, document)?;
    ensure!(
        cx.update(|cx| preferences::load(directory.path(), cx))
            .await
            .is_err()
    );
    ensure!(cx.read_global::<Preferences, _>(|p, _| p.request.proxy.validate().is_err()));

    certificate_round_trip(cx).await
}

async fn certificate_round_trip(cx: &mut AsyncApp) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let passphrase = "request-eagle-smoke-passphrase";
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await?;
    // Adding a certificate checks that the passphrase opens its file.
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["mtls.example.invalid".into()])?;
    let mut store = p12_keystore::KeyStore::new();
    store.add_entry(
        "client",
        p12_keystore::KeyStoreEntry::PrivateKeyChain(p12_keystore::PrivateKeyChain::new(
            vec![1],
            p12_keystore::PrivateKey::from_der(&signing_key.serialize_der())?,
            [p12_keystore::Certificate::from_der(cert.der())?],
        )),
    );
    let pkcs12 = directory.path().join("client.p12");
    fs::write(&pkcs12, store.writer(passphrase).write()?)?;
    let certificate = ClientCertificate {
        id: String::new(),
        host: "mtls.example.invalid".into(),
        files: CertificateFiles::Pkcs12 { path: pkcs12 },
        has_passphrase: false,
        passphrase: passphrase.into(),
        passphrase_unavailable: false,
    };
    println!("Saving a synthetic certificate passphrase to the native store");
    cx.update(|cx| preferences::add_client_certificate(certificate, cx))
        .await?;
    let path = directory.path().join("preferences.json");
    let document = fs::read_to_string(&path)?;
    ensure!(!document.contains(passphrase));
    let id = cx.read_global::<Preferences, _>(|p, _| p.request.client_certificates[0].id.clone());

    println!("Reloading the certificate passphrase from the native store");
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await?;
    ensure!(cx.read_global::<Preferences, _>(
        |p, _| p.request.client_certificates[0].passphrase == passphrase
    ));
    let file = preferences::PreferencesFile::new(directory.path());
    ensure!(file.request_preferences().await?.client_certificates[0].passphrase == passphrase);

    println!("Deleting the certificate passphrase from the native store");
    file.remove_client_certificate(&id).await?;
    ensure!(file.read()?.request.client_certificates.is_empty());

    // Reload the old reference to verify deletion, not just the JSON update.
    fs::write(&path, document)?;
    cx.update(|cx| preferences::load(directory.path(), cx))
        .await?;
    ensure!(cx.read_global::<Preferences, _>(|p, _| {
        p.request.client_certificates[0].passphrase_unavailable
    }));
    ensure!(file.request_preferences().await?.client_certificates[0].passphrase_unavailable);
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

    let file = preferences::PreferencesFile::new(directory.path());
    ensure!(
        file.update_proxy(
            |proxy| {
                proxy.authentication = true;
                Ok(())
            },
            Some(("headless-user".into(), "headless-password".into())),
        )
        .await
        .is_err()
    );
    ensure!(fs::read(&path)? == original);

    // Simulate a saved credential whose provider was removed between launches.
    let mut document: serde_json::Value = serde_json::from_slice(&original)?;
    document["proxy_credentials_id"] = uuid::Uuid::new_v4().to_string().into();
    document["request"]["proxy"]["authentication"] = true.into();
    fs::write(&path, serde_json::to_vec(&document)?)?;
    ensure!(file.request_preferences().await.is_err());
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
