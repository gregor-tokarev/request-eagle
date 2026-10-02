use request::{Field, Method};
use std::collections::HashMap;

#[test]
fn templated_header_names_defer_potentially_overridden_defaults() {
    let request = request::HttpRequest {
        path: "http://example.com".into(),
        method: Method::Post,
        headers: vec![Field::new("{{header_name}}", "virtual.example")],
        body: Some(request::Body::json("{}")),
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
            .push(Field::new("AUTHORIZATION", "Bearer explicit"));
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

        request.headers.push(Field::new("hOsT", "override.example"));
        assert!(
            super::execution::generated_headers(&request)
                .iter()
                .all(|(name, _)| name != "Host")
        );
    }
}

#[test]
fn body_types_preview_their_content_type_and_length() {
    use request::{Body, FormPart, RawLanguage};

    let preview = |method, body| {
        let request = request::HttpRequest {
            method,
            path: "https://example.com".into(),
            body: Some(body),
            ..Default::default()
        };
        let headers = super::execution::generated_headers(&request);
        let header = |name: &str| {
            headers
                .iter()
                .find(|(generated, _)| generated == name)
                .map(|(_, value)| value.clone())
        };

        (header("Content-Type"), header("Content-Length"))
    };
    let some =
        |content_type: &str, length: &str| (Some(content_type.to_owned()), Some(length.to_owned()));

    assert_eq!(
        preview(
            Method::Post,
            Body::Raw {
                language: RawLanguage::Xml,
                text: "<a/>".into(),
            }
        ),
        some("application/xml", "4")
    );
    // Empty text is not sent.
    assert_eq!(
        preview(Method::Post, Body::json("")),
        (None, Some("0".to_owned()))
    );
    assert_eq!(
        preview(
            Method::Put,
            Body::UrlEncoded {
                fields: vec![("a b".into(), "&".into())],
            }
        ),
        some("application/x-www-form-urlencoded", "Calculated on Send")
    );
    assert_eq!(
        preview(
            Method::Delete,
            Body::Multipart {
                parts: vec![FormPart {
                    name: "avatar".into(),
                    value: "eagle.png".into(),
                    file: true,
                }],
            }
        ),
        some(
            "multipart/form-data; boundary=Calculated on Send",
            "Calculated on Send"
        )
    );
    assert_eq!(
        preview(
            Method::Patch,
            Body::Binary {
                file: "eagle.png".into(),
            }
        ),
        some("image/png", "Calculated on Send")
    );
    assert_eq!(
        preview(
            Method::Get,
            Body::Binary {
                file: "eagle.png".into(),
            }
        ),
        (None, None)
    );
}
