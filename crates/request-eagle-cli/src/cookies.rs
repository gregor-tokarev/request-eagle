use anyhow::{Context as _, Result, bail};
use request::{Cookie, CookieJar};
use serde_json::{Value, json};
use std::{path::Path, time::UNIX_EPOCH};

use crate::commands::Command;

/// The app's cookie jar, which saved requests share when they run.
pub fn open(path: &Path) -> Result<CookieJar> {
    CookieJar::open(path).with_context(|| {
        format!(
            "Could not read the saved cookies in {}. Delete the file to start without cookies",
            path.display()
        )
    })
}

pub fn dispatch(path: &Path, command: Command) -> Result<Value> {
    let jar = open(path)?;
    let in_domain =
        |cookie: &Cookie, domain: &str| cookie.domain.eq_ignore_ascii_case(domain.trim());

    match command {
        Command::CookiesList { domain } => {
            let cookies = jar
                .cookies()
                .iter()
                .filter(|cookie| {
                    domain
                        .as_deref()
                        .is_none_or(|domain| in_domain(cookie, domain))
                })
                .map(cookie_json)
                .collect::<Vec<_>>();

            Ok(json!(cookies))
        }
        Command::CookiesDelete { domain, name } => {
            let deleted = jar
                .cookies()
                .into_iter()
                .filter(|cookie| {
                    in_domain(cookie, &domain)
                        && name.as_deref().is_none_or(|name| cookie.name == name)
                })
                .collect::<Vec<_>>();

            for cookie in &deleted {
                jar.remove(cookie);
            }
            jar.save().context("Could not save cookies")?;

            Ok(json!({"deleted": deleted.iter().map(cookie_json).collect::<Vec<_>>()}))
        }
        _ => bail!("Expected a cookies operation"),
    }
}

fn cookie_json(cookie: &Cookie) -> Value {
    json!({
        "domain": cookie.domain,
        "include_subdomains": !cookie.host_only,
        "path": cookie.path,
        "name": cookie.name,
        "value": cookie.value,
        "expires": cookie
            .expires
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs()),
        "secure": cookie.secure,
        "http_only": cookie.http_only,
    })
}
