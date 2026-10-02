//! OAuth 1.0 request signing (RFC 5849).

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::{hmac, signature};

use super::credentials::{Credential, Outgoing};
use super::crypto::{encode, hmac, random_token, rsa_sign};
use super::{AuthLocation, OAuth1Auth, OAuth1Signature};

pub(super) fn sign(
    auth: &OAuth1Auth,
    request: &Outgoing,
    now: SystemTime,
) -> Result<Vec<Credential>, String> {
    let timestamp = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    sign_with(auth, request, &timestamp, &random_token(24))
}

pub(super) fn sign_with(
    auth: &OAuth1Auth,
    request: &Outgoing,
    timestamp: &str,
    nonce: &str,
) -> Result<Vec<Credential>, String> {
    let mut oauth: Vec<(&str, String)> = vec![
        ("oauth_consumer_key", auth.consumer_key.clone()),
        ("oauth_nonce", nonce.to_owned()),
        (
            "oauth_signature_method",
            auth.signature_method.label().to_owned(),
        ),
        ("oauth_timestamp", timestamp.to_owned()),
        ("oauth_version", "1.0".to_owned()),
    ];
    for (name, value) in [
        ("oauth_token", &auth.access_token),
        ("oauth_callback", &auth.callback_url),
        ("oauth_verifier", &auth.verifier),
    ] {
        if !value.is_empty() {
            oauth.push((name, value.clone()));
        }
    }

    // The query, form fields and protocol parameters, encoded and sorted.
    let mut parameters: Vec<(String, String)> = request
        .url
        .query_pairs()
        .map(|(name, value)| (encode(&name), encode(&value)))
        .chain(
            request
                .form
                .iter()
                .map(|(name, value)| (encode(name), encode(value))),
        )
        .chain(
            oauth
                .iter()
                .map(|(name, value)| (encode(name), encode(value))),
        )
        .collect();
    parameters.sort();
    let parameters = parameters
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    let url = request.url;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let base_url = format!(
        "{}://{}{port}{}",
        url.scheme(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.path()
    );
    let base = format!(
        "{}&{}&{}",
        request.method.to_ascii_uppercase(),
        encode(&base_url),
        encode(&parameters)
    );
    let key = format!(
        "{}&{}",
        encode(&auth.consumer_secret),
        encode(&auth.token_secret)
    );

    let signed = |algorithm| STANDARD.encode(hmac(algorithm, key.as_bytes(), base.as_bytes()));
    let signature = match auth.signature_method {
        OAuth1Signature::HmacSha1 => signed(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
        OAuth1Signature::HmacSha256 => signed(hmac::HMAC_SHA256),
        OAuth1Signature::HmacSha512 => signed(hmac::HMAC_SHA512),
        OAuth1Signature::Plaintext => key.clone(),
        OAuth1Signature::RsaSha256 | OAuth1Signature::RsaSha512 => {
            let padding = if auth.signature_method == OAuth1Signature::RsaSha256 {
                &signature::RSA_PKCS1_SHA256
            } else {
                &signature::RSA_PKCS1_SHA512
            };
            let signature = rsa_sign(&auth.private_key, padding, base.as_bytes())
                .map_err(|error| format!("Could not sign the OAuth 1.0 request: {error}"))?;

            STANDARD.encode(signature)
        }
    };
    oauth.push(("oauth_signature", signature));

    Ok(match auth.add_to {
        AuthLocation::Header => {
            let mut fields = Vec::new();
            if !auth.realm.is_empty() {
                fields.push(format!("realm=\"{}\"", encode(&auth.realm)));
            }
            fields.extend(
                oauth
                    .iter()
                    .map(|(name, value)| format!("{name}=\"{}\"", encode(value))),
            );

            vec![Credential::Header(
                "Authorization".into(),
                format!("OAuth {}", fields.join(", ")),
            )]
        }
        AuthLocation::Query => oauth
            .into_iter()
            .map(|(name, value)| Credential::Query(name.to_owned(), value))
            .collect(),
    })
}
