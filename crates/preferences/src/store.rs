use crate::credentials::{CredentialStore, NativeCredentialStore, ProxyCredentials};
use crate::{AppearancePreferences, ProxyPreferences, RequestPreferences};

use anyhow::{Context as _, Result, anyhow, bail};
use gpui_kit::{App, Global, Task};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    rc::Rc,
};
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub appearance: AppearancePreferences,
    pub request: RequestPreferences,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) proxy_credentials_id: Option<String>,
}

impl Global for Preferences {}

struct Storage {
    path: Option<PathBuf>,
    load_error: Option<String>,
    credential_error: Option<String>,
    legacy_credentials: bool,
    credentials: Rc<dyn CredentialStore>,
    proxy_writes: Rc<async_lock::Mutex<()>>,
    proxy_revision: u64,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            path: None,
            load_error: None,
            credential_error: None,
            legacy_credentials: false,
            credentials: Rc::new(NativeCredentialStore),
            proxy_writes: Rc::default(),
            proxy_revision: 0,
        }
    }
}

impl Global for Storage {}

pub fn init(cx: &mut App) {
    if !cx.has_global::<Preferences>() {
        cx.set_global(Preferences::default());
    }

    if !cx.has_global::<Storage>() {
        cx.set_global(Storage::default());
    }
}

pub fn credential_error(cx: &App) -> Option<&str> {
    cx.try_global::<Storage>()?.credential_error.as_deref()
}

/// Load before opening the workspace. Keyring operations run off the UI thread.
pub fn load(directory: impl AsRef<Path>, cx: &mut App) -> Task<Result<()>> {
    init(cx);

    let path = directory.as_ref().join("preferences.json");
    let storage = cx.global_mut::<Storage>();
    storage.path = Some(path.clone());
    storage.load_error = None;
    storage.credential_error = None;
    storage.legacy_credentials = false;

    let result = read(&path).and_then(|bytes| {
        let preferences: Preferences = match bytes {
            Some(bytes) => serde_json::from_slice(&bytes).context("Invalid preferences.json")?,
            None => Preferences::default(),
        };

        if let Some(id) = &preferences.proxy_credentials_id {
            Uuid::parse_str(id).context("Invalid proxy credential reference")?;
        }

        Ok(preferences)
    });
    let preferences = match result {
        Ok(preferences) => preferences,
        Err(error) => {
            cx.global_mut::<Storage>().load_error = Some(format!("{error:#}"));
            return Task::ready(Err(error));
        }
    };
    let proxy = &preferences.request.proxy;
    let legacy = !proxy.username.is_empty() || !proxy.password.is_empty();
    let id = preferences.proxy_credentials_id.clone();
    cx.global_mut::<Storage>().legacy_credentials = legacy;
    cx.set_global(preferences);

    cx.spawn(async move |cx| {
        let result = if legacy {
            // The plaintext document remains untouched until the keyring write succeeds.
            let task =
                cx.update(|cx| update_proxy(cx.global::<Preferences>().request.proxy.clone(), cx));
            task.await
        } else if let Some(id) = id {
            let task = cx.update(|cx| cx.global::<Storage>().credentials.read(&id, cx));
            match decode_credentials(task.await) {
                Ok(credentials) => {
                    cx.update(|cx| {
                        let mut preferences = cx.global::<Preferences>().clone();
                        preferences.request.proxy.username = credentials.username;
                        preferences.request.proxy.password = credentials.password;
                        cx.set_global(preferences);
                    });
                    Ok(())
                }
                Err(error) => Err(error),
            }
        } else {
            Ok(())
        };

        if let Err(error) = &result {
            cx.update(|cx| {
                cx.global_mut::<Storage>().credential_error = Some(error.to_string());
                let mut preferences = cx.global::<Preferences>().clone();
                preferences.request.proxy.credentials_unavailable = true;
                cx.set_global(preferences);
            });
        }

        result
    })
}

/// Persist ordinary preferences before publishing them. Credential changes use
/// update_proxy so that keyring access never blocks the UI thread.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Preferences)) -> Result<()> {
    init(cx);
    check_load_error(cx)?;

    let previous = cx.global::<Preferences>();
    let mut preferences = previous.clone();
    change(&mut preferences);

    if let Some(path) = &cx.global::<Storage>().path {
        if cx.global::<Storage>().legacy_credentials {
            bail!(
                "Unlock your credential store and retry in Settings > Proxy before saving preferences."
            );
        }

        if preferences.request.proxy != previous.request.proxy {
            bail!("Proxy settings must be saved through the credential store.");
        }

        persist(path, &preferences)?;
    }

    cx.set_global(preferences);
    Ok(())
}

/// Serialize proxy edits, stage a new encrypted entry, then atomically replace
/// the JSON reference. A failed file save cannot damage the previous credentials.
pub fn update_proxy(mut proxy: ProxyPreferences, cx: &mut App) -> Task<Result<()>> {
    init(cx);
    let storage = cx.global_mut::<Storage>();
    storage.proxy_revision += 1;
    let revision = storage.proxy_revision;
    let writes = storage.proxy_writes.clone();

    // Detach the actual write from the caller: closing the editor must not cancel
    // a transaction after the OS has accepted its new secret.
    let (send, receive) = futures_channel::oneshot::channel();
    cx.spawn(async move |cx| {
        let _guard = writes.lock().await;

        // Coalesce drafts still waiting for the keyring. Correctness must not
        // depend on the executor polling foreground tasks in submission order.
        if cx.read_global::<Storage, _>(|storage, _| storage.proxy_revision != revision) {
            let _ = send.send(Ok(()));
            return;
        }

        let result: Result<()> = async {
            let (previous, path, credentials, legacy) = cx.update(|cx| {
                check_load_error(cx)?;
                let storage = cx.global::<Storage>();
                Ok::<_, anyhow::Error>((
                    cx.global::<Preferences>().clone(),
                    storage.path.clone(),
                    storage.credentials.clone(),
                    storage.legacy_credentials,
                ))
            })?;
            let old_proxy = &previous.request.proxy;
            let mut changed =
                proxy.username != old_proxy.username || proxy.password != old_proxy.password;

            // Retry a failed startup read without replacing inaccessible secrets
            // with empty inputs. Disabling proxy authentication still works.
            if proxy.credentials_unavailable && !old_proxy.credentials_unavailable && !legacy {
                // An earlier queued edit already restored the secret. This draft
                // still has the empty fields from before that read completed.
                proxy.username = old_proxy.username.clone();
                proxy.password = old_proxy.password.clone();
                proxy.credentials_unavailable = false;
                changed = false;
            } else if old_proxy.credentials_unavailable && !changed && !legacy {
                if proxy.mode == crate::ProxyMode::Custom && proxy.authentication {
                    if let Some(id) = &previous.proxy_credentials_id {
                        let task = cx.update(|cx| credentials.read(id, cx));
                        let restored = decode_credentials(task.await)?;
                        proxy.username = restored.username;
                        proxy.password = restored.password;
                    }
                    proxy.credentials_unavailable = false;
                } else {
                    proxy.credentials_unavailable = true;
                }
            } else {
                proxy.credentials_unavailable = false;
            }
            proxy.validate().map_err(|error| anyhow!(error))?;

            if proxy == previous.request.proxy && !legacy {
                return Ok(());
            }

            // Restoration above reuses the same entry.
            changed |= legacy;
            let new_id = if changed && path.is_some() {
                if proxy.username.is_empty() && proxy.password.is_empty() {
                    None
                } else {
                    Some(Uuid::new_v4().to_string())
                }
            } else {
                previous.proxy_credentials_id.clone()
            };
            let staged = changed && path.is_some() && new_id.is_some();

            if staged {
                let secret = serde_json::to_vec(&ProxyCredentials {
                    username: proxy.username.clone(),
                    password: proxy.password.clone(),
                })?;
                let task = cx.update(|cx| credentials.write(new_id.as_ref().unwrap(), &secret, cx));
                task.await?;
            }

            let saved = cx.update(|cx| {
                // Other settings can change while the OS keyring is open.
                let mut preferences = cx.global::<Preferences>().clone();
                preferences.request.proxy = proxy;
                preferences.proxy_credentials_id = new_id.clone();

                if let Some(path) = &path {
                    persist(path, &preferences)?;
                }

                let storage = cx.global_mut::<Storage>();
                storage.legacy_credentials = false;
                if !preferences.request.proxy.credentials_unavailable {
                    storage.credential_error = None;
                }
                cx.set_global(preferences);
                Ok(())
            });

            if saved.is_err() && staged {
                let _ = cx
                    .update(|cx| credentials.delete(new_id.as_ref().unwrap(), cx))
                    .await;
            } else if saved.is_ok()
                && previous.proxy_credentials_id != new_id
                && let Some(id) = &previous.proxy_credentials_id
            {
                // The new reference is already durable. Cleanup failures must
                // not roll it back; any orphan remains encrypted in the keyring.
                if cx.update(|cx| credentials.delete(id, cx)).await.is_err() {
                    eprintln!("Could not remove an unused encrypted proxy credential entry");
                }
            }

            saved
        }
        .await;
        cx.update(|cx| {
            if let Err(error) = &result {
                cx.global_mut::<Storage>().credential_error = Some(error.to_string());
            } else if !cx
                .global::<Preferences>()
                .request
                .proxy
                .credentials_unavailable
            {
                cx.global_mut::<Storage>().credential_error = None;
            }
        });
        let _ = send.send(result);
    })
    .detach();

    cx.spawn(async move |_| receive.await.context("Proxy save was interrupted")?)
}

fn check_load_error(cx: &App) -> Result<()> {
    if let Some(error) = &cx.global::<Storage>().load_error {
        bail!("{error}. Fix the preferences file and reload before saving changes.");
    }
    Ok(())
}

fn decode_credentials(result: Result<Option<Vec<u8>>>) -> Result<ProxyCredentials> {
    let secret = result?.context("Saved proxy credentials are missing from the keyring. Enter them again in Settings > Proxy.")?;
    serde_json::from_slice(&secret).map_err(|_| {
        anyhow!(
            "Saved proxy credentials could not be decoded. Enter them again in Settings > Proxy."
        )
    })
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Could not read {}", path.display())),
    }
}

fn persist(path: &Path, preferences: &Preferences) -> Result<()> {
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

#[cfg(test)]
pub(crate) fn set_credential_store(store: Rc<dyn CredentialStore>, cx: &mut App) {
    init(cx);
    cx.global_mut::<Storage>().credentials = store;
}
