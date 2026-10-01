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

#[test]
fn saving_keeps_the_cookies_another_process_saved_meanwhile() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cookies.json");
    let names = |jar: &CookieJar| {
        jar.cookies()
            .into_iter()
            .map(|cookie| cookie.name)
            .collect::<Vec<_>>()
    };

    let first = CookieJar::open(&path).unwrap();
    let second = CookieJar::open(&path).unwrap();
    store(&first, "https://example.com/", &["fast=1", "shared=first"]);
    first.save().unwrap();
    store(
        &second,
        "https://example.com/",
        &["slow=1", "shared=second"],
    );
    second.save().unwrap();

    assert_eq!(
        names(&CookieJar::open(&path).unwrap()),
        ["fast", "shared", "slow"]
    );
    assert_eq!(names(&second), ["fast", "shared", "slow"]);
    assert_eq!(
        CookieJar::open(&path)
            .unwrap()
            .cookie_header("https://example.com/", &[])
            .as_deref(),
        Some("fast=1; shared=second; slow=1")
    );

    // A deletion leaves the file, but the cookies this jar never had stay.
    first.remove(&first.cookies()[0]);
    first.save().unwrap();
    assert_eq!(names(&CookieJar::open(&path).unwrap()), ["shared", "slow"]);
}

#[test]
fn a_deletion_reaches_cookies_another_process_saved_meanwhile() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cookies.json");
    let logout = CookieJar::open(&path).unwrap();
    let login = CookieJar::open(&path).unwrap();

    store(&login, "https://example.com/", &["sid=1; Path=/"]);
    login.save().unwrap();
    store(
        &logout,
        "https://example.com/",
        &["sid=; Path=/; Max-Age=0"],
    );
    logout.save().unwrap();
    assert!(CookieJar::open(&path).unwrap().is_empty());

    // Setting the cookie again after a deletion keeps it.
    store(
        &logout,
        "https://example.com/",
        &["sid=; Path=/; Max-Age=0"],
    );
    store(&logout, "https://example.com/", &["sid=2; Path=/"]);
    logout.save().unwrap();
    assert_eq!(
        CookieJar::open(&path)
            .unwrap()
            .cookie_header("https://example.com/", &[])
            .as_deref(),
        Some("sid=2")
    );
}

#[test]
fn saving_reports_the_cookies_it_took_from_the_file_as_a_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cookies.json");
    let app = CookieJar::open(&path).unwrap();
    let cli = CookieJar::open(&path).unwrap();

    store(&cli, "https://example.com/", &["cli=1"]);
    cli.save().unwrap();
    store(&app, "https://example.org/", &["app=1"]);
    let revision = app.revision();
    app.save().unwrap();

    assert!(app.revision() > revision);
    assert_eq!(app.cookies().len(), 2);
    // The imported cookies are already saved.
    let revision = app.revision();
    app.save().unwrap();
    assert_eq!(app.revision(), revision);
}
