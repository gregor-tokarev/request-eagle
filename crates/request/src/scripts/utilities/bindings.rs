use rquickjs::{Ctx, Exception, Function, Object};

use super::{crypto, schema};

/// Native callbacks are captured by the sandbox rather than exposed as globals.
pub(in crate::scripts) fn bindings<'js>(cx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(cx.clone())?;

    object.set(
        "sha256",
        Function::new(cx.clone(), |cx: Ctx<'js>, text: String| {
            crypto::sha256(&text).map_err(|error| Exception::throw_type(&cx, &error))
        })?,
    )?;
    object.set(
        "hmacSha256",
        Function::new(cx.clone(), |cx: Ctx<'js>, secret: String, text: String| {
            crypto::hmac_sha256(&secret, &text).map_err(|error| Exception::throw_type(&cx, &error))
        })?,
    )?;
    object.set(
        "randomBytes",
        Function::new(cx.clone(), |cx: Ctx<'js>, count: f64| {
            crypto::random_bytes(count).map_err(|error| Exception::throw_range(&cx, &error))
        })?,
    )?;

    for (name, url_safe, encode) in [
        ("base64Encode", false, true),
        ("base64Decode", false, false),
        ("base64UrlEncode", true, true),
        ("base64UrlDecode", true, false),
    ] {
        object.set(
            name,
            Function::new(cx.clone(), move |cx: Ctx<'js>, text: String| {
                let result = if encode {
                    crypto::encode(&text, url_safe)
                } else {
                    crypto::decode(&text, url_safe)
                };

                result.map_err(|error| Exception::throw_type(&cx, &error))
            })?,
        )?;
    }

    for (name, convert) in [
        ("btoa", crypto::btoa as fn(&str) -> Result<String, String>),
        ("atob", crypto::atob),
    ] {
        object.set(
            name,
            Function::new(cx.clone(), move |cx: Ctx<'js>, text: String| {
                convert(&text).map_err(|error| Exception::throw_type(&cx, &error))
            })?,
        )?;
    }

    object.set(
        "validateSchema",
        Function::new(cx, |cx: Ctx<'js>, data: String, schema: String| {
            schema::validate(&data, &schema).map_err(|error| Exception::throw_type(&cx, &error))
        })?,
    )?;

    Ok(object)
}
