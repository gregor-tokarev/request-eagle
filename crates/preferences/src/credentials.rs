use anyhow::{Result, anyhow};
use gpui_kit::{App, Task};
use serde::{Deserialize, Serialize};

/// Both fields belong in the encrypted secret, not in searchable item attributes.
#[derive(Deserialize, Serialize)]
pub(crate) struct ProxyCredentials {
    pub username: String,
    pub password: String,
}

pub(crate) trait CredentialStore {
    fn read(&self, id: &str, cx: &App) -> Task<Result<Option<Vec<u8>>>>;
    fn write(&self, id: &str, secret: &[u8], cx: &App) -> Task<Result<()>>;
    fn delete(&self, id: &str, cx: &App) -> Task<Result<()>>;
}

pub(crate) struct NativeCredentialStore;

#[cfg(not(target_os = "linux"))]
impl CredentialStore for NativeCredentialStore {
    fn read(&self, id: &str, cx: &App) -> Task<Result<Option<Vec<u8>>>> {
        let task = cx.read_credentials(&format!("request-eagle.proxy/{id}"));

        cx.background_executor().spawn(async move {
            task.await
                .map(|entry| entry.map(|(_, secret)| secret))
                .map_err(|_| unavailable())
        })
    }

    fn write(&self, id: &str, secret: &[u8], cx: &App) -> Task<Result<()>> {
        let task = cx.write_credentials(&format!("request-eagle.proxy/{id}"), "proxy", secret);

        cx.background_executor()
            .spawn(async move { task.await.map_err(|_| unavailable()) })
    }

    fn delete(&self, id: &str, cx: &App) -> Task<Result<()>> {
        let task = cx.delete_credentials(&format!("request-eagle.proxy/{id}"));

        cx.background_executor()
            .spawn(async move { task.await.map_err(|_| unavailable()) })
    }
}

pub(crate) fn unavailable() -> anyhow::Error {
    if cfg!(target_os = "linux") {
        anyhow!(
            "Could not access the Secret Service keyring. Start and unlock GNOME Keyring or KWallet, then retry. Credentials were not saved to a plaintext file."
        )
    } else {
        anyhow!(
            "Could not access macOS Keychain. Unlock your login keychain and allow Request Eagle access, then retry. Credentials were not saved to a plaintext file."
        )
    }
}
