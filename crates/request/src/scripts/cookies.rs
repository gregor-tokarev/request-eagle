use rquickjs::{Ctx, Exception, Function};
use serde_json::json;
use url::Url;

use crate::{Cookie, CookieJar};

/// Lets `pm.cookies` read the jar's cookies for a URL and `pm.cookies.jar()`
/// change them. Without a jar, because the preferences turn it off, every
/// call throws.
pub(super) fn binding<'js>(
    cx: Ctx<'js>,
    jar: Option<CookieJar>,
) -> rquickjs::Result<Function<'js>> {
    Function::new(
        cx,
        move |cx: Ctx<'js>, operation: String, url: String, argument: String| {
            let Some(jar) = &jar else {
                return Err(Exception::throw_message(
                    &cx,
                    "The cookie jar is off. Turn it on in Settings to read and change cookies from scripts.",
                ));
            };
            let url = Url::parse(&url)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .ok_or_else(|| {
                    Exception::throw_type(
                        &cx,
                        &format!("Cookies need an http or https URL, not {url:?}"),
                    )
                })?;

            match operation.as_str() {
                "list" => {
                    let cookies = jar
                        .matching(&url)
                        .iter()
                        .map(script_cookie)
                        .collect::<Vec<_>>();

                    Ok(json!(cookies).to_string())
                }
                "stored" => {
                    let headers: Vec<String> = serde_json::from_str(&argument)
                        .map_err(|_| Exception::throw_type(&cx, "Expected Set-Cookie headers"))?;
                    let cookies = jar
                        .stored(&url, &headers)
                        .iter()
                        .map(script_cookie)
                        .collect::<Vec<_>>();

                    Ok(json!(cookies).to_string())
                }
                "set" => jar
                    .set(&url, &argument)
                    .map(|cookie| json!(cookie.as_ref().map(script_cookie)).to_string())
                    .map_err(|message| Exception::throw_message(&cx, &message)),
                "unset" => {
                    jar.unset(&url, Some(&argument));
                    Ok(String::new())
                }
                "clear" => {
                    jar.unset(&url, None);
                    Ok(String::new())
                }
                _ => Err(Exception::throw_type(&cx, "Unknown cookie operation")),
            }
        },
    )
}

/// A cookie as scripts see it, with its expiry in milliseconds.
fn script_cookie(cookie: &Cookie) -> serde_json::Value {
    json!({
        "name": cookie.name,
        "value": cookie.value,
        "domain": cookie.domain,
        "path": cookie.path,
        "hostOnly": cookie.host_only,
        "secure": cookie.secure,
        "httpOnly": cookie.http_only,
        "sameSite": cookie.same_site,
        "expires": cookie
            .expires
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as u64),
    })
}
