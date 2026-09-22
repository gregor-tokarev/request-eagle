use anyhow::Result;
use gpui_kit::{App, Task};

use crate::credentials::{CredentialStore, NativeCredentialStore, unavailable};

impl CredentialStore for NativeCredentialStore {
    fn read(&self, id: &str, cx: &App) -> Task<Result<Option<Vec<u8>>>> {
        let id = id.to_owned();

        cx.background_executor().spawn(async move {
            let result: Result<_> = async {
                let keyring = oo7::Keyring::new().await?;
                keyring.unlock().await?;
                let items = keyring.search_items(&attributes(&id)).await?;

                match items.first() {
                    Some(item) => {
                        item.unlock().await?;
                        Ok(Some(item.secret().await?.to_vec()))
                    }
                    None => Ok(None),
                }
            }
            .await;

            result.map_err(|_| unavailable())
        })
    }

    fn write(&self, id: &str, secret: &[u8], cx: &App) -> Task<Result<()>> {
        let id = id.to_owned();
        let secret = secret.to_vec();

        cx.background_executor().spawn(async move {
            let result: Result<()> = async {
                let keyring = oo7::Keyring::new().await?;
                keyring.unlock().await?;
                keyring
                    .create_item("Request Eagle proxy", &attributes(&id), secret, true)
                    .await?;
                Ok(())
            }
            .await;

            result.map_err(|_| unavailable())
        })
    }

    fn delete(&self, id: &str, cx: &App) -> Task<Result<()>> {
        let id = id.to_owned();

        cx.background_executor().spawn(async move {
            let result: Result<()> = async {
                let keyring = oo7::Keyring::new().await?;
                keyring.unlock().await?;
                keyring.delete(&attributes(&id)).await?;
                Ok(())
            }
            .await;

            result.map_err(|_| unavailable())
        })
    }
}

fn attributes(id: &str) -> [(&str, &str); 2] {
    [
        ("application", "request-eagle"),
        ("proxy-credentials-id", id),
    ]
}
