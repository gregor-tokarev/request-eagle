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

#[test]
fn a_script_that_sets_a_deleted_cookie_again_keeps_it_saved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cookies.json");
    let jar = CookieJar::open(&path).unwrap();
    let url = Url::parse("https://example.com/").unwrap();

    store(&jar, url.as_str(), &["sid=1; Path=/"]);
    jar.save().unwrap();
    store(&jar, url.as_str(), &["sid=; Path=/; Max-Age=0"]);
    let restored = jar.set(&url, "sid=1; Path=/").unwrap().unwrap();
    assert_eq!(restored.value, "1");
    jar.save().unwrap();

    assert_eq!(
        CookieJar::open(&path)
            .unwrap()
            .cookie_header(url.as_str(), &[])
            .as_deref(),
        Some("sid=1")
    );
    assert_eq!(
        jar.cookie_header(url.as_str(), &[]).as_deref(),
        Some("sid=1")
    );

    // A script's deletion also reaches the file.
    jar.unset(&url, Some("sid"));
    jar.save().unwrap();
    assert!(CookieJar::open(&path).unwrap().is_empty());
}

#[test]
fn the_cookie_url_ends_at_the_query_outside_variable_names() {
    let values = std::collections::HashMap::from([
        ("host?name".to_owned(), "example.com".to_owned()),
        ("section".to_owned(), "admin".to_owned()),
    ]);
    let sent = |path: &str| {
        crate::variables::sent_url(path, &[], &values, &std::collections::BTreeMap::new())
    };

    assert_eq!(
        sent("https://{{host?name}}/{{section}}?q={{unset}}").as_deref(),
        Some("https://example.com/admin")
    );
    assert_eq!(
        sent("{{host?name}}/{{section}}#{{unset}}").as_deref(),
        Some("https://example.com/admin")
    );
    assert_eq!(sent("https://{{unset}}/admin"), None);
}

#[test]
fn a_copy_keeps_its_own_changes() {
    let jar = CookieJar::new();
    store(&jar, "https://example.com/", &["session=1"]);

    let copy = jar.copy();
    store(&copy, "https://example.com/", &["session=2", "added=3"]);

    assert_eq!(
        jar.cookie_header("https://example.com/", &[]).as_deref(),
        Some("session=1")
    );
    assert_eq!(
        copy.cookie_header("https://example.com/", &[]).as_deref(),
        Some("session=2; added=3")
    );
}

#[test]
fn extending_replaces_cookies_of_the_same_name_and_keeps_others() {
    let jar = CookieJar::new();
    store(&jar, "https://example.com/", &["session=1", "theme=dark"]);
    let run = CookieJar::new();
    store(&run, "https://example.com/", &["session=2"]);
    store(&run, "https://other.example/", &["id=3"]);
    let revision = jar.revision();

    jar.extend(&run);

    assert!(jar.revision() > revision);
    assert_eq!(
        jar.cookie_header("https://example.com/", &[]).as_deref(),
        Some("session=2; theme=dark")
    );
    assert_eq!(
        jar.cookie_header("https://other.example/", &[]).as_deref(),
        Some("id=3")
    );
}
