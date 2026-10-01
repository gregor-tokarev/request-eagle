#[cfg(feature = "ui")]
use anyhow::Result;
use anyhow::anyhow;
#[cfg(feature = "ui")]
use gpui_kit::{App, Task};
use serde::{Deserialize, Serialize};

/// Both fields belong in the encrypted secret, not in searchable item attributes.
#[derive(Deserialize, Serialize)]
pub(crate) struct ProxyCredentials {
    pub username: String,
    pub password: String,
}

/// What a secret in the OS credential store is for, which names its entry.
#[derive(Clone, Copy)]
pub(crate) enum Secret {
    ProxyCredentials,
    CertificatePassphrase,
}

#[cfg(feature = "ui")]
pub(crate) trait CredentialStore {
    fn read(&self, secret: Secret, id: &str, cx: &App) -> Task<Result<Option<Vec<u8>>>>;
    fn write(&self, secret: Secret, id: &str, value: &[u8], cx: &App) -> Task<Result<()>>;
    fn delete(&self, secret: Secret, id: &str, cx: &App) -> Task<Result<()>>;
}

#[cfg(feature = "ui")]
pub(crate) struct NativeCredentialStore;

#[cfg(feature = "ui")]
impl CredentialStore for NativeCredentialStore {
    fn read(&self, secret: Secret, id: &str, cx: &App) -> Task<Result<Option<Vec<u8>>>> {
        let id = id.to_owned();
        cx.background_executor()
            .spawn(async move { read(secret, &id).await })
    }

    fn write(&self, secret: Secret, id: &str, value: &[u8], cx: &App) -> Task<Result<()>> {
        let id = id.to_owned();
        let value = value.to_vec();
        cx.background_executor()
            .spawn(async move { write(secret, &id, &value).await })
    }

    fn delete(&self, secret: Secret, id: &str, cx: &App) -> Task<Result<()>> {
        let id = id.to_owned();
        cx.background_executor()
            .spawn(async move { delete(secret, &id).await })
    }
}

#[cfg(target_os = "linux")]
pub(crate) use crate::linux_credentials::{delete, read, write};
#[cfg(target_os = "macos")]
pub(crate) use crate::macos_credentials::{delete, read, write};

pub(crate) fn unavailable() -> anyhow::Error {
    if cfg!(target_os = "linux") {
        anyhow!(
            "Could not access a Secret Service keyring. Start and unlock your provider (KeePassXC, GNOME Keyring, or KWallet) in this desktop session, then retry. KeePassXC requires Secret Service integration enabled. Credentials were not saved to a plaintext file."
        )
    } else {
        anyhow!(
            "Could not access macOS Keychain. Unlock your login keychain and allow Request Eagle access, then retry. Credentials were not saved to a plaintext file."
        )
    }
}
