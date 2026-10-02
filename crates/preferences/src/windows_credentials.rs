use std::{ffi::c_void, ptr, slice};

use anyhow::Result;
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    },
};

use crate::credentials::{Secret, unavailable};

/// Generic credentials in Windows Credential Manager, named like the macOS
/// Keychain entries.
fn target(secret: Secret, id: &str) -> Vec<u16> {
    let account = match secret {
        Secret::ProxyCredentials => "proxy",
        Secret::CertificatePassphrase => "certificate",
    };

    format!("request-eagle.{account}/{id}")
        .encode_utf16()
        .chain([0])
        .collect()
}

pub(crate) async fn read(secret: Secret, id: &str) -> Result<Option<Vec<u8>>> {
    let target = target(secret, id);

    smol::unblock(move || {
        let mut credential: *mut CREDENTIALW = ptr::null_mut();

        // SAFETY: The target is NUL-terminated, and a successful read returns
        // a credential that is copied and then released with CredFree.
        unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                return if GetLastError() == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(unavailable())
                };
            }

            let value = slice::from_raw_parts(
                (*credential).CredentialBlob,
                (*credential).CredentialBlobSize as usize,
            )
            .to_vec();
            CredFree(credential as *const c_void);

            Ok(Some(value))
        }
    })
    .await
}

pub(crate) async fn write(secret: Secret, id: &str, value: &[u8]) -> Result<()> {
    let mut target = target(secret, id);
    let mut value = value.to_vec();

    smol::unblock(move || {
        // SAFETY: Every pointer in the credential outlives the call, and
        // CredWriteW copies what it stores.
        unsafe {
            let mut credential: CREDENTIALW = std::mem::zeroed();
            credential.Type = CRED_TYPE_GENERIC;
            credential.TargetName = target.as_mut_ptr();
            credential.CredentialBlobSize = value.len() as u32;
            credential.CredentialBlob = value.as_mut_ptr();
            credential.Persist = CRED_PERSIST_LOCAL_MACHINE;

            if CredWriteW(&credential, 0) == 0 {
                Err(unavailable())
            } else {
                Ok(())
            }
        }
    })
    .await
}

pub(crate) async fn delete(secret: Secret, id: &str) -> Result<()> {
    let target = target(secret, id);

    smol::unblock(move || {
        // SAFETY: The target is NUL-terminated.
        unsafe {
            if CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) == 0
                && GetLastError() != ERROR_NOT_FOUND
            {
                Err(unavailable())
            } else {
                Ok(())
            }
        }
    })
    .await
}
