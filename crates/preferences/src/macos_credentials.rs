use anyhow::Result;
use core_foundation::{base::TCFType, string::CFString};
use security_framework::passwords::{self, PasswordOptions};
use security_framework_sys::{base::errSecItemNotFound, item::*};

use crate::credentials::unavailable;

// Preserve GPUI's internet-password attributes for existing desktop entries.
#[allow(deprecated)]
fn options(id: &str) -> PasswordOptions {
    PasswordOptions {
        query: unsafe {
            vec![
                (
                    CFString::wrap_under_get_rule(kSecClass),
                    CFString::wrap_under_get_rule(kSecClassInternetPassword).into_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrServer),
                    CFString::new(&format!("request-eagle.proxy/{id}")).into_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrAccount),
                    CFString::new("proxy").into_CFType(),
                ),
            ]
        },
    }
}

pub(crate) async fn read(id: &str) -> Result<Option<Vec<u8>>> {
    let id = id.to_owned();

    smol::unblock(move || match passwords::generic_password(options(&id)) {
        Ok(secret) => Ok(Some(secret)),
        Err(error) if error.code() == errSecItemNotFound => Ok(None),
        Err(_) => Err(unavailable()),
    })
    .await
}

pub(crate) async fn write(id: &str, secret: &[u8]) -> Result<()> {
    let id = id.to_owned();
    let secret = secret.to_vec();

    smol::unblock(move || {
        passwords::set_generic_password_options(&secret, options(&id)).map_err(|_| unavailable())
    })
    .await
}

pub(crate) async fn delete(id: &str) -> Result<()> {
    let id = id.to_owned();

    smol::unblock(
        move || match passwords::delete_generic_password_options(options(&id)) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == errSecItemNotFound => Ok(()),
            Err(_) => Err(unavailable()),
        },
    )
    .await
}
