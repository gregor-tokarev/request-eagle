//! Hashing, signing and encoding shared by the authorizations that sign
//! requests.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use ring::{
    hmac,
    rand::{SecureRandom as _, SystemRandom},
    signature::{self, EcdsaKeyPair, RsaKeyPair},
};
use rustls::pki_types::{PrivateKeyDer, pem::PemObject};

/// Characters left as they are by RFC 3986 percent-encoding, which OAuth 1.0
/// and AWS signatures use.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(super) fn encode(text: &str) -> String {
    utf8_percent_encode(text, UNRESERVED).to_string()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn sha256(data: &[u8]) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, data)
        .as_ref()
        .to_vec()
}

pub(super) fn hmac(algorithm: hmac::Algorithm, key: &[u8], data: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(algorithm, key), data)
        .as_ref()
        .to_vec()
}

/// Random bytes as URL-safe Base64, for nonces, states and PKCE verifiers.
pub(super) fn random_token(bytes: usize) -> String {
    let mut random = vec![0; bytes];
    SystemRandom::new()
        .fill(&mut random)
        .expect("the system's random number generator");

    URL_SAFE_NO_PAD.encode(random)
}

/// Sign with the RSA key in a PEM file's text.
pub(super) fn rsa_sign(
    pem: &str,
    padding: &'static dyn signature::RsaEncoding,
    message: &[u8],
) -> Result<Vec<u8>, String> {
    let key = match private_key(pem)? {
        PrivateKeyDer::Pkcs1(key) => RsaKeyPair::from_der(key.secret_pkcs1_der()),
        PrivateKeyDer::Pkcs8(key) => RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()),
        _ => return Err("the private key is not an RSA key".into()),
    }
    .map_err(|error| format!("the private key is not a usable RSA key: {error}"))?;

    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(padding, &SystemRandom::new(), message, &mut signature)
        .map_err(|_| "could not sign with the RSA key".to_owned())?;

    Ok(signature)
}

/// Sign with the elliptic curve key in a PEM file's text, as JSON Web
/// Signatures expect: the two numbers of the signature side by side.
pub(super) fn ecdsa_sign(
    pem: &str,
    algorithm: &'static signature::EcdsaSigningAlgorithm,
    message: &[u8],
) -> Result<Vec<u8>, String> {
    let PrivateKeyDer::Pkcs8(key) = private_key(pem)? else {
        return Err(
            "the private key must be in PKCS #8 form (BEGIN PRIVATE KEY). Convert it with: openssl pkcs8 -topk8 -nocrypt -in key.pem"
                .into(),
        );
    };
    let random = SystemRandom::new();
    let key = EcdsaKeyPair::from_pkcs8(algorithm, key.secret_pkcs8_der(), &random)
        .map_err(|error| format!("the private key does not match the algorithm: {error}"))?;

    key.sign(&random, message)
        .map(|signature| signature.as_ref().to_vec())
        .map_err(|_| "could not sign with the private key".to_owned())
}

fn private_key(pem: &str) -> Result<PrivateKeyDer<'static>, String> {
    if pem.trim().is_empty() {
        return Err("enter a private key".into());
    }

    PrivateKeyDer::from_pem_slice(pem.trim().as_bytes())
        .map_err(|_| "the private key is not an unencrypted PEM key".to_owned())
}
