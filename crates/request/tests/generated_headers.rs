use request::{Method, generated_headers};

#[test]
fn previews_only_headers_the_transport_adds() {
    assert_eq!(
        generated_headers(Method::Get, "https://example.com/path", &[], 0),
        [
            ("Host".into(), "example.com".into()),
            ("Accept".into(), "*/*".into()),
            ("Accept-Encoding".into(), "gzip".into())
        ]
    );

    for method in [Method::Post, Method::Put, Method::Patch] {
        let headers = generated_headers(method, "http://[::1]:8080/path", &[], 0);
        assert_eq!(headers[0], ("Host".into(), "[::1]:8080".into()));
        assert_eq!(headers[3], ("Content-Length".into(), "0".into()));
    }

    let body = "{\"name\":\"🦅\"}";
    let headers = generated_headers(Method::Post, "https://example.com", &[], body.len());
    assert_eq!(
        headers[3],
        ("Content-Length".into(), body.len().to_string())
    );
    assert_eq!(
        generated_headers(Method::Get, "", &[], 0),
        [
            ("Accept".into(), "*/*".into()),
            ("Accept-Encoding".into(), "gzip".into())
        ]
    );
}

#[test]
fn explicit_headers_override_defaults_case_insensitively() {
    let headers = [
        ("hOsT".into(), "virtual.example".into()),
        ("ACCEPT".into(), "application/json".into()),
        ("AcCePt-EnCoDiNg".into(), "identity".into()),
        ("content-LENGTH".into(), "12".into()),
        ("AUTHORIZATION".into(), "Bearer example".into()),
    ];
    assert!(
        generated_headers(Method::Post, "http://user:pass@example.com", &headers, 12).is_empty()
    );

    let headers = [("Transfer-Encoding".into(), "chunked".into())];
    assert!(
        !generated_headers(Method::Post, "http://example.com", &headers, 12)
            .iter()
            .any(|(name, _)| name == "Content-Length")
    );
}

#[test]
fn range_header_suppresses_generated_accept_encoding_case_insensitively() {
    let headers = [("rAnGe".into(), "bytes=0-9".into())];

    assert_eq!(
        generated_headers(Method::Get, "https://example.com/path", &headers, 0),
        [
            ("Host".into(), "example.com".into()),
            ("Accept".into(), "*/*".into())
        ]
    );
}

#[test]
fn shows_url_credentials_as_authorization_without_leaking_them_into_host() {
    let headers = generated_headers(
        Method::Get,
        "https://user:p%40ss@example.com:8443/path",
        &[],
        0,
    );
    assert_eq!(headers[0], ("Host".into(), "example.com:8443".into()));
    assert_eq!(
        headers[1],
        ("Authorization".into(), "Basic dXNlcjpwQHNz".into())
    );
}
