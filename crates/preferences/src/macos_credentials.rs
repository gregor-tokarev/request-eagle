use anyhow::Result;
use core_foundation::{base::TCFType, string::CFString};
use security_framework::passwords::{self, PasswordOptions};
use security_framework_sys::{base::errSecItemNotFound, item::*};

use crate::credentials::{Secret, unavailable};

// Preserve GPUI's internet-password attributes for existing desktop entries.
#[allow(deprecated)]
fn options(secret: Secret, id: &str) -> PasswordOptions {
    let account = match secret {
        Secret::ProxyCredentials => "proxy",
        Secret::CertificatePassphrase => "certificate",
    };

    PasswordOptions {
        query: unsafe {
            vec![
                (
                    CFString::wrap_under_get_rule(kSecClass),
                    CFString::wrap_under_get_rule(kSecClassInternetPassword).into_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrServer),
                    CFString::new(&format!("request-eagle.{account}/{id}")).into_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrAccount),
                    CFString::new(account).into_CFType(),
                ),
            ]
        },
    }
}

pub(crate) async fn read(secret: Secret, id: &str) -> Result<Option<Vec<u8>>> {
    let id = id.to_owned();

    smol::unblock(
        move || match passwords::generic_password(options(secret, &id)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code() == errSecItemNotFound => Ok(None),
            Err(_) => Err(unavailable()),
        },
    )
    .await
}

pub(crate) async fn write(secret: Secret, id: &str, value: &[u8]) -> Result<()> {
    let id = id.to_owned();
    let value = value.to_vec();

    smol::unblock(move || {
        passwords::set_generic_password_options(&value, options(secret, &id))
            .map_err(|_| unavailable())
    })
    .await
}

pub(crate) async fn delete(secret: Secret, id: &str) -> Result<()> {
    let id = id.to_owned();

    smol::unblock(
        move || match passwords::delete_generic_password_options(options(secret, &id)) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == errSecItemNotFound => Ok(()),
            Err(_) => Err(unavailable()),
        },
    )
    .await
}
