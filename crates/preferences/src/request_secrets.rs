use std::collections::HashMap;

use anyhow::Result;
use gpui_kit::{App, Task};

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
