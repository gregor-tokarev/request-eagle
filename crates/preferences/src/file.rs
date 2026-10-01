use crate::{AppearancePreferences, ProxyPreferences, RequestPreferences};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// Which releases the app updates to. Daily builds are published as GitHub
/// pre-releases; stable releases are the ones promoted from them.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Daily,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub appearance: AppearancePreferences,
    pub request: RequestPreferences,
    pub vim_mode: bool,
    pub update_channel: UpdateChannel,
    /// The global environment selected in the workspace, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) proxy_credentials_id: Option<String>,
}

/// File-backed preferences for clients that do not have an application context.
/// Each operation reads a fresh document; proxy secrets remain in the OS store.
pub struct PreferencesFile {
    path: PathBuf,
}

impl PreferencesFile {
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("preferences.json"),
        }
    }

    /// Read configuration without opening the keyring or returning saved secrets.
    pub fn read(&self) -> Result<Preferences> {
        let mut preferences = read_document(&self.path)?;
        preferences.request.proxy.username.clear();
        preferences.request.proxy.password.clear();
        Ok(preferences)
    }

    pub fn update(
        &self,
        change: impl FnOnce(&mut Preferences) -> Result<()>,
    ) -> Result<Preferences> {
        let _lock = self.lock()?;
        let mut preferences = read_document(&self.path)?;
        let previous_proxy = preferences.request.proxy.clone();

        if !previous_proxy.username.is_empty() || !previous_proxy.password.is_empty() {
            bail!(
                "Migrate legacy proxy credentials through Settings > Proxy before editing other preferences."
            );
        }

        change(&mut preferences)?;
        if preferences.request.proxy != previous_proxy {
            bail!("Use update_proxy to change proxy settings.");
        }

        persist(&self.path, &preferences)?;
        Ok(preferences)
    }

    /// Resolve credentials only when an authenticated custom proxy is in use.
    pub async fn request_preferences(&self) -> Result<RequestPreferences> {
        let mut preferences = read_document(&self.path)?;
        let proxy = &mut preferences.request.proxy;

        if proxy.mode == crate::ProxyMode::Custom
            && proxy.authentication
            && let Some(id) = &preferences.proxy_credentials_id
        {
            let secret = crate::credentials::read(id)
                .await?
                .context("Saved proxy credentials are missing from the keyring")?;
            let credentials: crate::credentials::ProxyCredentials = serde_json::from_slice(&secret)
                .map_err(|_| anyhow::anyhow!("Saved proxy credentials could not be decoded"))?;
            proxy.username = credentials.username;
            proxy.password = credentials.password;
        }

        proxy.validate().map_err(anyhow::Error::msg)?;
        Ok(preferences.request)
    }

    /// Patch proxy configuration. Supply both credential fields to replace them;
    /// an endpoint change without replacement credentials revokes authentication.
    pub async fn update_proxy(
        &self,
        change: impl FnOnce(&mut ProxyPreferences) -> Result<()>,
        credentials: Option<(String, String)>,
    ) -> Result<Preferences> {
        let _lock = self.lock()?;
        let mut preferences = read_document(&self.path)?;
        let old_proxy = preferences.request.proxy.clone();
        let old_id = preferences.proxy_credentials_id.clone();
        let proxy = &mut preferences.request.proxy;
        change(proxy)?;

        if proxy.username != old_proxy.username || proxy.password != old_proxy.password {
            bail!("Use the credentials argument to replace proxy credentials.");
        }

        let endpoint_changed = old_proxy.host.trim().to_lowercase()
            != proxy.host.trim().to_lowercase()
            || old_proxy.port != proxy.port
            || old_proxy.protocol != proxy.protocol;
        let legacy = !old_proxy.username.is_empty() || !old_proxy.password.is_empty();
        let credentials = credentials.or_else(|| {
            (legacy && !endpoint_changed)
                .then(|| (old_proxy.username.clone(), old_proxy.password.clone()))
        });

        if endpoint_changed {
            preferences.proxy_credentials_id = None;
            proxy.username.clear();
            proxy.password.clear();
            if credentials.is_none() {
                proxy.authentication = false;
            }
        }

        let mut staged = None;
        if let Some((username, password)) = credentials {
            proxy.username = username;
            proxy.password = password;
            preferences.proxy_credentials_id = None;
            if !proxy.username.is_empty() || !proxy.password.is_empty() {
                let id = Uuid::new_v4().to_string();
                let secret = serde_json::to_vec(&crate::credentials::ProxyCredentials {
                    username: proxy.username.clone(),
                    password: proxy.password.clone(),
                })?;
                staged = Some((id.clone(), secret));
                preferences.proxy_credentials_id = Some(id);
            }
        }

        proxy.validate().map_err(anyhow::Error::msg)?;
        if let Some((id, secret)) = &staged {
            crate::credentials::write(id, secret).await?;
        }

        proxy.username.clear();
        proxy.password.clear();

        if let Err(error) = persist(&self.path, &preferences) {
            if let Some((id, _)) = &staged {
                let _ = crate::credentials::delete(id).await;
            }
            return Err(error);
        }

        if old_id != preferences.proxy_credentials_id
            && let Some(id) = old_id
        {
            let _ = crate::credentials::delete(&id).await;
        }

        Ok(preferences)
    }

    fn lock(&self) -> Result<fs::File> {
        let directory = self
            .path
            .parent()
            .context("Preferences path has no parent")?;
        fs::create_dir_all(directory)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("preferences.lock"))?;
        lock.try_lock()
            .context("Preferences are being edited by another process; retry")?;
        Ok(lock)
    }
}

/// Reads preferences as saved, including any legacy plaintext proxy credentials.
/// A missing file yields the defaults.
pub(crate) fn read_document(path: &Path) -> Result<Preferences> {
    let preferences: Preferences = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid preferences.json")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
        Err(error) => {
            return Err(error).with_context(|| format!("Could not read {}", path.display()));
        }
    };

    if let Some(id) = &preferences.proxy_credentials_id {
        Uuid::parse_str(id).context("Invalid proxy credential reference")?;
    }

    Ok(preferences)
}

pub(crate) fn persist(path: &Path, preferences: &Preferences) -> Result<()> {
    let parent = path.parent().context("Preferences path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(preferences)?)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .with_context(|| format!("Could not save {}", path.display()))?;
    Ok(())
}
