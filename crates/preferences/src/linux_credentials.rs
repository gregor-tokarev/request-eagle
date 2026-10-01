use anyhow::Result;

use crate::credentials::{Secret, unavailable};

pub(crate) async fn read(secret: Secret, id: &str) -> Result<Option<Vec<u8>>> {
    let result: Result<_> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        let items = keyring.search_items(&attributes(secret, id)).await?;

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

pub(crate) async fn write(secret: Secret, id: &str, value: &[u8]) -> Result<()> {
    let result: Result<()> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        keyring
            .create_item(label(secret), &attributes(secret, id), value, true)
            .await?;
        Ok(())
    }
    .await;

    result.map_err(|_| unavailable())
}

pub(crate) async fn delete(secret: Secret, id: &str) -> Result<()> {
    let result: Result<()> = async {
        let keyring = oo7::Keyring::new().await?;
        keyring.unlock().await?;
        keyring.delete(&attributes(secret, id)).await?;
        Ok(())
    }
    .await;

    result.map_err(|_| unavailable())
}

fn label(secret: Secret) -> &'static str {
    match secret {
        Secret::ProxyCredentials => "Request Eagle proxy",
        Secret::CertificatePassphrase => "Request Eagle certificate passphrase",
    }
}

fn attributes(secret: Secret, id: &str) -> [(&str, &str); 2] {
    let key = match secret {
        Secret::ProxyCredentials => "proxy-credentials-id",
        Secret::CertificatePassphrase => "certificate-passphrase-id",
    };

    [("application", "request-eagle"), (key, id)]
}
