use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};

const TEXT_LIMIT: usize = 1024 * 1024;
const RANDOM_LIMIT: usize = 65_536;

fn check_text(text: &str) -> Result<(), String> {
    if text.len() > TEXT_LIMIT {
        return Err("Crypto and encoding inputs cannot exceed 1 MiB".into());
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }

    result
}

pub(super) fn sha256(text: &str) -> Result<String, String> {
    check_text(text)?;

    Ok(hex(
        digest::digest(&digest::SHA256, text.as_bytes()).as_ref()
    ))
}

pub(super) fn hmac_sha256(secret: &str, text: &str) -> Result<String, String> {
    check_text(secret)?;
    check_text(text)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());

    Ok(hex(hmac::sign(&key, text.as_bytes()).as_ref()))
}

pub(super) fn random_bytes(count: f64) -> Result<String, String> {
    if !count.is_finite() || count.fract() != 0. || !(0. ..=RANDOM_LIMIT as f64).contains(&count) {
        return Err("randomBytes requires an integer between 0 and 65536".into());
    }

    let mut bytes = vec![0; count as usize];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "Unable to generate secure random bytes".to_owned())?;

    Ok(hex(&bytes))
}

pub(super) fn encode(text: &str, url_safe: bool) -> Result<String, String> {
    check_text(text)?;
    let engine = if url_safe { URL_SAFE_NO_PAD } else { STANDARD };

    Ok(engine.encode(text.as_bytes()))
}

pub(super) fn decode(text: &str, url_safe: bool) -> Result<String, String> {
    if text.len() > TEXT_LIMIT.div_ceil(3) * 4 {
        return Err("Decoded base64 cannot exceed 1 MiB".into());
    }

    let engine = if url_safe {
        if text.ends_with('=') {
            URL_SAFE
        } else {
            URL_SAFE_NO_PAD
        }
    } else {
        STANDARD
    };
    let bytes = engine
        .decode(text)
        .map_err(|_| "Invalid base64 input".to_owned())?;

    if bytes.len() > TEXT_LIMIT {
        return Err("Decoded base64 cannot exceed 1 MiB".into());
    }

    String::from_utf8(bytes).map_err(|_| "Decoded base64 is not valid UTF-8".into())
}
