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
