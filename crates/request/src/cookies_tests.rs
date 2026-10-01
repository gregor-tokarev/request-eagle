use http_client::http::{HeaderMap, HeaderValue, header::SET_COOKIE};
use url::Url;

use crate::CookieJar;

fn store(jar: &CookieJar, url: &str, cookies: &[&str]) {
    let mut headers = HeaderMap::new();

    for cookie in cookies {
        headers.append(SET_COOKIE, HeaderValue::from_str(cookie).unwrap());
    }

    jar.store(&Url::parse(url).unwrap(), &headers);
}

#[test]
fn domain_cookies_reach_subdomains_but_not_other_sites() {
    let jar = CookieJar::new();
    store(
        &jar,
        "https://login.example.com/",
        &["shared=1; Domain=example.com", "own=2"],
    );

    assert_eq!(
        jar.cookie_header("https://api.example.com/", &[])
            .as_deref(),
        Some("shared=1")
    );
    assert_eq!(
        jar.cookie_header("https://login.example.com/", &[])
            .as_deref(),
        Some("shared=1; own=2")
    );
    assert_eq!(jar.cookie_header("https://example.org/", &[]), None);
}

#[test]
fn refuses_cookies_for_a_whole_top_level_domain() {
    let jar = CookieJar::new();
    store(
        &jar,
        "https://example.com/",
        &["wide=1; Domain=com", "other=1; Domain=localhost"],
    );
    assert!(jar.is_empty());

    // A single-label host can still set cookies for itself.
    store(&jar, "http://localhost:8080/", &["dev=1; Domain=localhost"]);
    assert_eq!(
        jar.cookie_header("http://localhost:3000/", &[]).as_deref(),
        Some("dev=1")
    );
}

#[test]
fn secure_cookies_are_sent_only_over_https() {
    let jar = CookieJar::new();
    store(&jar, "https://example.com/", &["token=1; Secure"]);

    assert_eq!(jar.cookie_header("http://example.com/", &[]), None);
    assert_eq!(
        jar.cookie_header("https://example.com/", &[]).as_deref(),
        Some("token=1")
    );
}
