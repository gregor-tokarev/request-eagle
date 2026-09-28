use anyhow::Result;

use crate::credentials::unavailable;

pub(crate) async fn read(id: &str) -> Result<Option<Vec<u8>>> {
    let result: Result<_> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        let items = keyring.search_items(&attributes(id)).await?;

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
}

pub(crate) async fn write(id: &str, secret: &[u8]) -> Result<()> {
    let result: Result<()> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        keyring
            .create_item("Request Eagle proxy", &attributes(id), secret, true)
            .await?;
        Ok(())
    }
    .await;

    result.map_err(|_| unavailable())
}

pub(crate) async fn delete(id: &str) -> Result<()> {
    let result: Result<()> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        keyring.delete(&attributes(id)).await?;
        Ok(())
    }
    .await;

    result.map_err(|_| unavailable())
}

fn attributes(id: &str) -> [(&str, &str); 2] {
    [
        ("application", "request-eagle"),
        ("proxy-credentials-id", id),
    ]
}
