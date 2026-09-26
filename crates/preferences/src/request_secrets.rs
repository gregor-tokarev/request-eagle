use std::{collections::HashMap, fs::OpenOptions};

use anyhow::Result;
use gpui_kit::{App, Task};

/// Apply one edit to the current keyring contents, rather than a UI snapshot.
/// A changed name with no value override moves the latest stored value.
pub fn update_request_secret(
    previous_name: Option<String>,
    name: String,
    value: Option<String>,
    cx: &mut App,
) -> Task<Result<HashMap<String, String>>> {
    let lock = cx.background_executor().spawn(async {
        let directory = std::env::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not locate the request secret lock directory."))?
            .join(".request-eagle");
        std::fs::create_dir_all(&directory)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(directory.join("request-secrets.lock"))?;
        file.lock()?;
        Ok::<_, anyhow::Error>(file)
    });
    cx.spawn(async move |cx| {
        // Keep the per-user lock through the entire keyring read and replacement.
        // The file contains no secret data; closing it releases the OS lock.
        let _lock = lock.await?;
        let mut values = cx.update(|cx| read_request_secrets(cx)).await?;
        let mut value = value;
        if let Some(previous) = previous_name.filter(|previous| previous != &name) {
            anyhow::ensure!(
                !values.contains_key(&name),
                "A variable with that name already exists."
            );
            let current = values.remove(&previous).ok_or_else(|| {
                anyhow::anyhow!("The selected variable was removed. Reload its source and retry.")
            })?;
            value = Some(value.unwrap_or(current));
        }
        if let Some(value) = value {
            values.insert(name, value);
        } else {
            values.remove(&name);
        }
        cx.update(|cx| write_request_secrets(&values, cx)).await?;
        Ok(values)
    })
}

/// Request secrets use their own keyring item, independent of proxy credentials.
pub fn read_request_secrets(cx: &App) -> Task<Result<HashMap<String, String>>> {
    #[cfg(not(target_os = "linux"))]
    let read = cx.read_credentials("request-eagle.request-secrets");

    cx.background_executor().spawn(async move {
        #[cfg(target_os = "linux")]
        let bytes = async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring
                .search_items(&[
                    ("application", "request-eagle"),
                    ("kind", "request-secrets"),
                ])
                .await?;
            match items.first() {
                Some(item) => {
                    item.unlock().await?;
                    Ok::<_, anyhow::Error>(Some(item.secret().await?.to_vec()))
                }
                None => Ok(None),
            }
        }
        .await
        .map_err(|_| crate::credentials::unavailable())?;

        #[cfg(not(target_os = "linux"))]
        let bytes = read
            .await
            .map_err(|_| crate::credentials::unavailable())?
            .map(|(_, bytes)| bytes);

        bytes
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map(|values| values.unwrap_or_default())
            .map_err(|_| anyhow::anyhow!("Could not read request secrets from the keyring."))
    })
}

pub fn write_request_secrets(values: &HashMap<String, String>, cx: &App) -> Task<Result<()>> {
    let bytes = serde_json::to_vec(values).expect("string map serializes");
    #[cfg(not(target_os = "linux"))]
    let write = cx.write_credentials("request-eagle.request-secrets", "secrets", &bytes);

    cx.background_executor().spawn(async move {
        #[cfg(target_os = "linux")]
        let result: Result<()> = async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            keyring
                .create_item(
                    "Request Eagle secrets",
                    &[
                        ("application", "request-eagle"),
                        ("kind", "request-secrets"),
                    ],
                    bytes,
                    true,
                )
                .await?;
            Ok(())
        }
        .await;

        #[cfg(not(target_os = "linux"))]
        let result = write.await;

        result.map_err(|_| crate::credentials::unavailable())
    })
}
