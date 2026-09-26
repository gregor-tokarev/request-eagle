use environment::VariableValues;
use request::{HttpRequest, Method};

#[test]
fn resolves_every_request_field_in_a_snapshot() {
    let values = VariableValues {
        environment: [
            ("base_url".into(), "https://example.com".into()),
            ("key".into(), "message".into()),
            ("value".into(), "🦅 hello".into()),
        ]
        .into(),
        secrets: [("token".into(), "demo-token".into())].into(),
    };
    let draft = HttpRequest {
        method: Method::Post,
        path: "{{base_url}}/echo?id={{$guid}}".into(),
        headers: vec![
            ("X-{{key}}".into(), "{{value}}".into()),
            ("Authorization".into(), "Bearer {{vault:token}}".into()),
        ],
        query: Some(vec![("{{key}}".into(), "{{$guid}}".into())]),
        body: Some(br#"{"value":"{{value}}","id":"{{$guid}}"}"#.to_vec()),
    };
    let before = draft.clone();
    let outgoing = draft.resolve_variables(&values).unwrap();
    assert_eq!(draft, before);
    assert!(outgoing.path.starts_with("https://example.com/echo?id="));
    assert_eq!(outgoing.headers[0], ("X-message".into(), "🦅 hello".into()));
    assert_eq!(outgoing.headers[1].1, "Bearer demo-token");
    let id = &outgoing.query.as_ref().unwrap()[0].1;
    assert!(outgoing.path.ends_with(id));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(outgoing.body.as_ref().unwrap()).unwrap()["id"],
        *id
    );
}
#[test]
fn url_fragments_do_not_resolve_or_validate_unsent_references() {
    let values = environment::VariableValues {
        environment: [
            ("host".into(), "example.com".into()),
            ("base_url".into(), "https://example.com/path#local".into()),
            ("fragment".into(), "#local".into()),
        ]
        .into(),
        ..Default::default()
    };
    for (path, expected) in [
        ("https://example.com/#{{missing}}", "https://example.com/"),
        ("https://{{host}}/#{{unclosed", "https://example.com/"),
        ("{{base_url}}/{{missing}}", "https://example.com/path"),
        (
            "https://{{host}}/{{fragment}}/{{unclosed",
            "https://example.com/",
        ),
    ] {
        let request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        assert_eq!(request.resolve_variables(&values).unwrap().path, expected);
        assert_eq!(request.path, path, "retain the fragment in the draft");
    }

    for path in [
        "https://{{missing}}/#ignored",
        "https://example.com/%23{{missing}}",
        "https://{{unclosed#fragment}}",
    ] {
        let request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        assert!(
            request.resolve_variables(&values).is_err(),
            "{path} requires a value before the fragment"
        );
    }
    let request = request::HttpRequest {
        path: "https://example.com/".into(),
        headers: vec![("X-Value".into(), "#{{host}}".into())],
        body: Some(b"#{{host}}".to_vec()),
        ..Default::default()
    };
    let resolved = request.resolve_variables(&values).unwrap();
    assert_eq!(resolved.headers[0].1, "#example.com");
    assert_eq!(resolved.body.as_deref(), Some(b"#example.com".as_slice()));
}
