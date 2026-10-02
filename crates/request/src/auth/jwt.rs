//! JSON Web Tokens (RFC 7519) signed when a request is sent.

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ring::{hmac, signature};
use serde_json::{Map, Value};

use super::crypto::{ecdsa_sign, hmac, rsa_sign};
use super::{JwtAlgorithm, JwtAuth};

pub(super) fn token(auth: &JwtAuth) -> Result<String, String> {
    let payload = compact_json(&auth.payload)
        .map_err(|error| format!("The JWT payload is not valid JSON: {error}"))?;
    let header = header(auth)?;
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let signature =
        sign(auth, input.as_bytes()).map_err(|error| format!("Could not sign the JWT: {error}"))?;

    Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

/// `alg` and `typ`, followed by the fields the auth adds.
fn header(auth: &JwtAuth) -> Result<String, String> {
    let mut fields = vec![
        ("alg".to_owned(), Value::from(auth.algorithm.label())),
        ("typ".to_owned(), Value::from("JWT")),
    ];

    if !auth.headers.trim().is_empty() {
        let extra: Map<String, Value> = serde_json::from_str(&auth.headers)
            .map_err(|_| "The JWT headers are not a JSON object".to_owned())?;

        for (name, value) in extra {
            match fields.iter_mut().find(|(field, _)| *field == name) {
                // The algorithm is the one that signs the token.
                Some(_) if name == "alg" => {}
                Some((_, field)) => *field = value,
                None => fields.push((name, value)),
            }
        }
    }

    let fields = fields
        .iter()
        .map(|(name, value)| format!("{}:{value}", Value::from(name.as_str())))
        .collect::<Vec<_>>();

    Ok(format!("{{{}}}", fields.join(",")))
}

fn sign(auth: &JwtAuth, input: &[u8]) -> Result<Vec<u8>, String> {
    let rsa = |padding| rsa_sign(&auth.private_key, padding, input);
    let ecdsa = |algorithm| ecdsa_sign(&auth.private_key, algorithm, input);

    match auth.algorithm {
        JwtAlgorithm::Hs256 | JwtAlgorithm::Hs384 | JwtAlgorithm::Hs512 => {
            let secret = if auth.secret_base64 {
                STANDARD
                    .decode(auth.secret.trim())
                    .or_else(|_| URL_SAFE_NO_PAD.decode(auth.secret.trim()))
                    .map_err(|_| "the secret is not Base64".to_owned())?
            } else {
                auth.secret.clone().into_bytes()
            };
            let algorithm = match auth.algorithm {
                JwtAlgorithm::Hs256 => hmac::HMAC_SHA256,
                JwtAlgorithm::Hs384 => hmac::HMAC_SHA384,
                _ => hmac::HMAC_SHA512,
            };

            Ok(hmac(algorithm, &secret, input))
        }
        JwtAlgorithm::Rs256 => rsa(&signature::RSA_PKCS1_SHA256),
        JwtAlgorithm::Rs384 => rsa(&signature::RSA_PKCS1_SHA384),
        JwtAlgorithm::Rs512 => rsa(&signature::RSA_PKCS1_SHA512),
        JwtAlgorithm::Ps256 => rsa(&signature::RSA_PSS_SHA256),
        JwtAlgorithm::Ps384 => rsa(&signature::RSA_PSS_SHA384),
        JwtAlgorithm::Ps512 => rsa(&signature::RSA_PSS_SHA512),
        JwtAlgorithm::Es256 => ecdsa(&signature::ECDSA_P256_SHA256_FIXED_SIGNING),
        JwtAlgorithm::Es384 => ecdsa(&signature::ECDSA_P384_SHA384_FIXED_SIGNING),
    }
}

/// The JSON without the whitespace between its tokens, keeping the order
/// its fields were written in.
fn compact_json(text: &str) -> Result<String, serde_json::Error> {
    serde_json::from_str::<Value>(text)?;

    let mut compact = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;

    for character in text.chars() {
        if in_string {
            compact.push(character);
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
        } else if character == '"' {
            in_string = true;
            compact.push(character);
        } else if !character.is_whitespace() {
            compact.push(character);
        }
    }

    Ok(compact)
}
