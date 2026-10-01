use request::Method;
use std::collections::HashMap;

#[test]
fn templated_header_names_defer_potentially_overridden_defaults() {
    let request = request::HttpRequest {
        path: "http://example.com".into(),
        method: Method::Post,
        headers: vec![("{{header_name}}".into(), "virtual.example".into())],
        body: Some(b"{}".to_vec()),
        ..Default::default()
    };
    let preview = super::execution::generated_headers(&request);
    assert!(preview.iter().all(|(_, value)| value == "Resolved on Send"));

    for name in [
        "Host",
        "Accept",
        "Accept-Encoding",
        "Content-Length",
        "Content-Type",
    ] {
        let values = HashMap::from([("header_name".into(), name.into())]);
        let resolved = request.resolve_variables(&values).unwrap();
        assert!(
            super::execution::generated_headers(&resolved)
                .iter()
                .all(|(generated, _)| generated != name)
        );
    }
}

#[test]
fn templated_url_credentials_preview_authorization_as_unresolved() {
    for (path, expected) in [
        ("{{scheme}}://user:pass@example.com", "Resolved on Send"),
        ("{{base_url}}/users", "Resolved on Send"),
        ("https://{{authority}}/users", "Resolved on Send"),
        (
            "https://{{user}}:{{password}}@example.com",
            "Resolved on Send",
        ),
        ("https://user:{{password}}@example.com", "Resolved on Send"),
        (
            "https://{{user}}:pass@{{host}}:{{port}}",
            "Resolved on Send",
        ),
        (
            "https://user:pass@example.com/{{path}}",
            "Basic dXNlcjpwYXNz",
        ),
    ] {
        let mut request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        let headers = super::execution::generated_headers(&request);
        assert_eq!(
            headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.as_str()),
            Some(expected),
            "{path}"
        );

        request
            .headers
            .push(("AUTHORIZATION".into(), "Bearer explicit".into()));
        assert!(
            super::execution::generated_headers(&request)
                .iter()
                .all(|(name, _)| name != "Authorization")
        );
    }
    let request = request::HttpRequest {
        path: "{{scheme}}://example.com".into(),
        ..Default::default()
    };
    assert!(
        super::execution::generated_headers(&request)
            .iter()
            .all(|(name, _)| name != "Authorization")
    );
}

#[test]
fn templated_urls_preview_generated_host_without_hiding_known_hosts() {
    for (path, expected) in [
        ("{{base_url}}/users", "Resolved on Send"),
        ("https://{{host}}/users", "Resolved on Send"),
        ("https://example.com:{{port}}/users", "Resolved on Send"),
        ("https://example.com/{{path}}", "example.com"),
        ("example.com:8443/?q={{query}}", "example.com:8443"),
    ] {
        let mut request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        let headers = super::execution::generated_headers(&request);
        assert_eq!(headers[0], ("Host".into(), expected.into()), "{path}");

        request
            .headers
            .push(("hOsT".into(), "override.example".into()));
        assert!(
            super::execution::generated_headers(&request)
                .iter()
                .all(|(name, _)| name != "Host")
        );
    }
}
