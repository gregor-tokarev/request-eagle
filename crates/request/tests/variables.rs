use environment::VariableScopes;
use request::{Body, Field, HttpRequest, Method};
use std::collections::HashMap;

fn environment(changes: &[(&str, &str)]) -> VariableScopes {
    VariableScopes {
        environment: changes
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Some((*value).to_owned())))
            .collect(),
        ..Default::default()
    }
}

#[test]
fn session_values_resolve_across_request_snapshots_without_changing_drafts() {
    let session = environment::EnvironmentSession::default();
    let file_values = HashMap::from([("token".into(), "saved value".into())]);
    session
        .apply(&environment(&[("token", "response token")]))
        .unwrap();
    let draft = HttpRequest {
        path: "https://example.com".into(),
        headers: vec![Field::new("Authorization", "Bearer {{token}}")],
        ..Default::default()
    };

    let first = request::RequestVariables::with_environment_session(
        file_values.clone(),
        HashMap::new(),
        None,
        session.clone(),
    );
    assert_eq!(
        first.resolve(&draft).unwrap().headers[0].value,
        "Bearer response token"
    );
    session
        .apply(&environment(&[("token", "refreshed token")]))
        .unwrap();
    let next = request::RequestVariables::with_environment_session(
        file_values.clone(),
        HashMap::new(),
        None,
        session,
    );
    assert_eq!(
        next.resolve(&draft).unwrap().headers[0].value,
        "Bearer refreshed token"
    );
    assert_eq!(draft.headers[0].value, "Bearer {{token}}");
    assert_eq!(file_values["token"], "saved value");
}

#[test]
fn session_can_supply_values_when_the_environment_file_cannot_be_read() {
    let session = environment::EnvironmentSession::default();
    session
        .apply(&environment(&[("token", "session token")]))
        .unwrap();
    let variables = request::RequestVariables::with_environment_session(
        HashMap::new(),
        HashMap::new(),
        Some("Invalid environment file".into()),
        session,
    );
    let mut draft = HttpRequest {
        path: "https://example.com".into(),
        headers: vec![Field::new("Authorization", "Bearer {{token}}")],
        ..Default::default()
    };

    assert_eq!(
        variables.resolve(&draft).unwrap().headers[0].value,
        "Bearer session token"
    );
    draft.headers[0].value = "{{missing}}".into();
    assert_eq!(
        variables.resolve(&draft).unwrap_err(),
        "Invalid environment file"
    );
}

#[test]
fn escaped_references_remain_literal_in_every_request_field() {
    let values = HashMap::from([("customer".into(), "must not replace".into())]);
    let draft = HttpRequest {
        method: Method::Post,
        path: "https://example.com/{{!customer}}".into(),
        headers: vec![Field::new("X-{{!customer}}", "{{!missing}}")],
        query: vec![Field::new("{{!customer}}", "{{!$guid}}")],
        body: Some(Body::json(r#"{"template":"Hello {{!customer}}"}"#)),
        ..Default::default()
    };
    let resolved = draft.resolve_variables(&values).unwrap();
    assert_eq!(resolved.path, "https://example.com/{{customer}}");
    assert_eq!(
        resolved.headers[0],
        Field::new("X-{{customer}}", "{{missing}}")
    );
    assert_eq!(resolved.query[0], Field::new("{{customer}}", "{{$guid}}"));
    assert_eq!(
        resolved.body,
        Some(Body::json(r#"{"template":"Hello {{customer}}"}"#))
    );
}

#[test]
fn resolves_every_request_field_in_a_snapshot() {
    let values = HashMap::from([
        ("base_url".into(), "https://example.com".into()),
        ("key".into(), "message".into()),
        ("value".into(), "🦅 hello".into()),
    ]);
    let draft = HttpRequest {
        method: Method::Post,
        path: "{{base_url}}/echo?id={{$guid}}".into(),
        headers: vec![
            Field::new("X-{{key}}", "{{value}}"),
            Field::new("X-Request-ID", "{{$guid}}"),
        ],
        query: vec![Field::new("{{key}}", "{{$guid}}")],
        body: Some(Body::json(r#"{"value":"{{value}}","id":"{{$guid}}"}"#)),
        ..Default::default()
    };
    let before = draft.clone();
    let outgoing = draft.resolve_variables(&values).unwrap();
    assert_eq!(draft, before);
    assert!(outgoing.path.starts_with("https://example.com/echo?id="));
    assert_eq!(outgoing.headers[0], Field::new("X-message", "🦅 hello"));
    let id = &outgoing.query[0].value;
    assert!(outgoing.path.ends_with(id));
    assert_eq!(&outgoing.headers[1].value, id);
    assert_eq!(
        outgoing.body,
        Some(Body::json(format!(r#"{{"value":"🦅 hello","id":"{id}"}}"#)))
    );
}
#[test]
fn url_fragments_do_not_resolve_or_validate_unsent_references() {
    let values = HashMap::from([
        ("host".into(), "example.com".into()),
        ("base_url".into(), "https://example.com/path#local".into()),
        ("fragment".into(), "#local".into()),
    ]);
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
        headers: vec![Field::new("X-Value", "#{{host}}")],
        body: Some(Body::json("#{{host}}")),
        ..Default::default()
    };
    let resolved = request.resolve_variables(&values).unwrap();
    assert_eq!(resolved.headers[0].value, "#example.com");
    assert_eq!(resolved.body, Some(Body::json("#example.com")));
}
