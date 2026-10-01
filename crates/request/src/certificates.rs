//! Certificates read from files the user chose: certificate authorities to
//! trust, and client certificates presented to servers that ask for one.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
use serde::{Deserialize, Serialize};

/// A certificate presented to the servers of one host when they ask for one
/// (mutual TLS).
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ClientCertificate {
    /// Names the passphrase in the OS credential store.
    pub id: String,
    /// `api.example.com`, optionally with a port, as in `api.example.com:8443`.
    /// `*.example.com` matches its subdomains. Without a port, any port matches.
    pub host: String,
    pub files: CertificateFiles,
    /// Whether the OS credential store keeps a passphrase for the key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_passphrase: bool,
    /// Decrypts the key. Read from the OS credential store and never saved
    /// with preferences.
    #[serde(skip)]
    pub passphrase: String,
    /// The passphrase could not be read from the OS credential store.
    #[serde(skip)]
    pub passphrase_unavailable: bool,
}

/// Where a client certificate and its private key are stored.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum CertificateFiles {
    /// A PEM certificate, followed by any intermediates, and its PEM private
    /// key. Without a key file, the key is read from the certificate file.
    Pem {
        certificate: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<PathBuf>,
    },
    /// A PKCS #12 file (`.p12` or `.pfx`) with the certificate and its key.
    Pkcs12 { path: PathBuf },
}

impl CertificateFiles {
    /// The same files by absolute paths, which still find them from another
    /// working directory.
    pub fn absolute(self) -> std::io::Result<Self> {
        Ok(match self {
            Self::Pem { certificate, key } => Self::Pem {
                certificate: std::path::absolute(certificate)?,
                key: key.map(std::path::absolute).transpose()?,
            },
            Self::Pkcs12 { path } => Self::Pkcs12 {
                path: std::path::absolute(path)?,
            },
        })
    }
}

impl fmt::Debug for ClientCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCertificate")
            .field("id", &self.id)
            .field("host", &self.host)
            .field("files", &self.files)
            .field("has_passphrase", &self.has_passphrase)
            .finish_non_exhaustive()
    }
}

/// A client certificate chain and its private key.
pub(crate) struct Identity {
    pub(crate) chain: Vec<CertificateDer<'static>>,
    pub(crate) key: PrivateKeyDer<'static>,
}

impl ClientCertificate {
    pub fn validate(&self) -> Result<(), &'static str> {
        let host = self.host.trim();

        if host.is_empty() {
            return Err("Enter the host to send the certificate to.");
        }

        if host.contains("://")
            || host.contains(['/', '?', '#', '@'])
            || host.contains(char::is_whitespace)
        {
            return Err(
                "Enter only a host and optional port, such as api.example.com or *.example.com:8443.",
            );
        }

        let empty = |path: &Path| path.as_os_str().is_empty();
        match &self.files {
            CertificateFiles::Pem { certificate, .. } if empty(certificate) => {
                Err("Choose a certificate file.")
            }
            CertificateFiles::Pkcs12 { path } if empty(path) => Err("Choose a PKCS #12 file."),
            _ => Ok(()),
        }
    }

    /// How closely the certificate's host matches a connection to `host` on
    /// `port`, or `None` when it does not. An exact host is closer than a
    /// wildcard, and a matching port closer than any port.
    fn closeness(&self, host: &str, port: u16) -> Option<u8> {
        let (pattern, pattern_port) = split_port(self.host.trim());

        if pattern_port.is_some_and(|pattern_port| pattern_port != port) {
            return None;
        }

        // URLs write IPv6 addresses in brackets, which a host may leave out.
        let normalize = |host: &str| {
            host.trim_matches(['[', ']'])
                .trim_end_matches('.')
                .to_ascii_lowercase()
        };
        let pattern = normalize(pattern);
        let host = normalize(host);
        let exact = if pattern == host {
            true
        } else if let Some(domain) = pattern.strip_prefix("*.") {
            let subdomain = host.strip_suffix(domain)?.strip_suffix('.')?;
            if subdomain.is_empty() {
                return None;
            }

            false
        } else {
            return None;
        };

        Some(u8::from(exact) * 2 + u8::from(pattern_port.is_some()))
    }

    /// Read the files to check that they hold a certificate and its private
    /// key, which the passphrase decrypts.
    pub fn check(&self) -> Result<(), String> {
        let identity = self.load()?;

        rustls::sign::CertifiedKey::from_der(
            identity.chain,
            identity.key,
            &rustls::crypto::ring::default_provider(),
        )
        .map(|_| ())
        .map_err(key_error)
    }

    /// Read the certificate chain and its key, decrypting the key with the
    /// passphrase.
    pub(crate) fn load(&self) -> Result<Identity, String> {
        let identity = match &self.files {
            CertificateFiles::Pem { certificate, key } => {
                let pem = read(certificate)?;
                let chain = CertificateDer::pem_slice_iter(&pem)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| {
                        format!("{} is not a valid PEM file: {error}", certificate.display())
                    })?;

                if chain.is_empty() {
                    return Err(format!("{} has no PEM certificate", certificate.display()));
                }

                let key = match key {
                    Some(path) => self.private_key(&read(path)?, path)?,
                    None => self.private_key(&pem, certificate)?,
                };

                Identity { chain, key }
            }
            CertificateFiles::Pkcs12 { path } => {
                self.check_passphrase()?;

                let store = p12_keystore::KeyStore::from_pkcs12(
                    &read(path)?,
                    &self.passphrase,
                    p12_keystore::Pkcs12ImportPolicy::Relaxed,
                )
                .map_err(|error| match error {
                    p12_keystore::error::Error::MacError(_) => {
                        format!(
                            "could not open {}: {}",
                            path.display(),
                            self.wrong_passphrase()
                        )
                    }
                    error => format!("could not open {}: {error}", path.display()),
                })?;
                let (_, chain) = store
                    .private_key_chain()
                    .ok_or_else(|| format!("{} has no private key", path.display()))?;

                if chain.certs().is_empty() {
                    return Err(format!("{} has no certificate", path.display()));
                }

                Identity {
                    chain: chain
                        .certs()
                        .iter()
                        .map(|certificate| CertificateDer::from(certificate.as_der().to_vec()))
                        .collect(),
                    key: PrivatePkcs8KeyDer::from(chain.key().as_der().to_vec()).into(),
                }
            }
        };

        Ok(identity)
    }

    fn private_key(&self, pem: &[u8], path: &Path) -> Result<PrivateKeyDer<'static>, String> {
        if let Ok(key) = PrivateKeyDer::from_pem_slice(pem) {
            return Ok(key);
        }

        let text = String::from_utf8_lossy(pem);

        if let Some(encrypted) = pem_section(&text, "ENCRYPTED PRIVATE KEY") {
            self.check_passphrase()?;

            let invalid = || format!("{} has an invalid encrypted private key", path.display());
            let der = STANDARD.decode(encrypted).map_err(|_| invalid())?;
            let info = pkcs8::EncryptedPrivateKeyInfoRef::try_from(der.as_slice())
                .map_err(|_| invalid())?;
            let key = info.decrypt(&self.passphrase).map_err(|_| {
                format!(
                    "could not decrypt the private key in {}: {}",
                    path.display(),
                    self.wrong_passphrase()
                )
            })?;

            return Ok(PrivatePkcs8KeyDer::from(key.as_bytes().to_vec()).into());
        }

        if text.contains("Proc-Type: 4,ENCRYPTED") {
            return Err(format!(
                "the private key in {} is encrypted in the legacy OpenSSL format. Convert it to PKCS #8 with: openssl pkcs8 -topk8 -in {0} -out key.pem",
                path.display()
            ));
        }

        Err(format!("{} has no PEM private key", path.display()))
    }

    fn wrong_passphrase(&self) -> &'static str {
        if self.passphrase.is_empty() {
            "it is protected by a passphrase; enter it"
        } else {
            "the passphrase is incorrect"
        }
    }

    fn check_passphrase(&self) -> Result<(), String> {
        if self.passphrase_unavailable {
            return Err(
                "the certificate's passphrase is unavailable. Unlock your keyring, then restart Request Eagle".into(),
            );
        }

        Ok(())
    }
}

/// Explain why a certificate and key could not be used together.
pub(crate) fn key_error(error: rustls::Error) -> String {
    match error {
        rustls::Error::InconsistentKeys(_) => {
            "the private key does not belong to the certificate".into()
        }
        error => error.to_string(),
    }
}

/// The certificate to present to `host` on `port`: the closest match, or
/// the first of equally close ones.
pub(crate) fn client_certificate<'a>(
    certificates: &'a [ClientCertificate],
    host: &str,
    port: u16,
) -> Option<&'a ClientCertificate> {
    certificates
        .iter()
        .rev()
        .filter_map(|certificate| Some((certificate.closeness(host, port)?, certificate)))
        .max_by_key(|(closeness, _)| *closeness)
        .map(|(_, certificate)| certificate)
}

/// The certificates in a PEM file of certificate authorities.
pub(crate) fn ca_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let certificates = CertificateDer::pem_slice_iter(&read(path)?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{} is not a valid PEM file: {error}", path.display()))?;

    if certificates.is_empty() {
        return Err(format!("{} has no PEM certificates", path.display()));
    }

    Ok(certificates)
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

/// Split a trailing `:port`, which an IPv6 address has only in brackets.
fn split_port(host: &str) -> (&str, Option<u16>) {
    if let Some((name, port)) = host.rsplit_once(':')
        && (name.ends_with(']') || !name.contains(':'))
        && let Ok(port) = port.parse()
    {
        return (name, Some(port));
    }

    (host, None)
}

/// The base64 body of the first PEM section with `label`.
fn pem_section(text: &str, label: &str) -> Option<String> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text.find(&begin)? + begin.len();
    let length = text[start..].find(&end)?;

    Some(text[start..start + length].split_whitespace().collect())
}
