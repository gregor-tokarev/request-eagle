use std::collections::HashMap;

use request::{
    ApiKeyLocation, Authentication, HttpRequest, Method, VariableError, resolve_variables,
};

fn variables() -> HashMap<String, String> {
    HashMap::from([
        ("base_url".into(), "https://example.test".into()),
        ("header".into(), "X-Account".into()),
        ("account".into(), "bird & eagle".into()),
        ("key".into(), "account".into()),
        ("token".into(), "secret-value".into()),
    ])
}

#[test]
fn resolves_all_request_fields_without_mutating_the_template() {
    let template = HttpRequest {
        method: Method::Post,
        path: "{{ base_url }}/{{key}}".into(),
        headers: vec![("{{header}}".into(), "{{account}}: {{account}}".into())],
        query: Some(vec![("{{key}}".into(), "{{account}}".into())]),
        body: Some(b"{\"account\":\"{{account}}\"}".to_vec()),
        authentication: Authentication::Bearer {
            token: "{{token}}".into(),
        },
    };
    let resolved = resolve_variables(&template, &variables()).unwrap();

    assert_eq!(resolved.path, "https://example.test/account");
    assert_eq!(
        resolved.headers,
        [("X-Account".into(), "bird & eagle: bird & eagle".into())]
    );
    assert_eq!(
        resolved.query,
        Some(vec![("account".into(), "bird & eagle".into())])
    );
    assert_eq!(resolved.body.unwrap(), b"{\"account\":\"bird & eagle\"}");
    assert_eq!(
        resolved.authentication,
        Authentication::Bearer {
            token: "secret-value".into(),
        }
    );
    assert_eq!(template.path, "{{ base_url }}/{{key}}");
    assert_eq!(template.headers[0].0, "{{header}}");
    assert_eq!(template.body.unwrap(), b"{\"account\":\"{{account}}\"}");
    assert_eq!(
        template.authentication,
        Authentication::Bearer {
            token: "{{token}}".into(),
        }
    );
}

#[test]
fn resolves_basic_and_api_key_credentials() {
    for (authentication, expected) in [
        (
            Authentication::Basic {
                username: "{{account}}".into(),
                password: "{{token}}".into(),
            },
            Authentication::Basic {
                username: "bird & eagle".into(),
                password: "secret-value".into(),
            },
        ),
        (
            Authentication::ApiKey {
                name: "{{header}}".into(),
                value: "{{token}}".into(),
                location: ApiKeyLocation::Header,
            },
            Authentication::ApiKey {
                name: "X-Account".into(),
                value: "secret-value".into(),
                location: ApiKeyLocation::Header,
            },
        ),
    ] {
        let request = HttpRequest {
            authentication,
            ..HttpRequest::default()
        };
        let resolved = resolve_variables(&request, &variables()).unwrap();

        assert_eq!(resolved.authentication, expected);
    }
}

#[test]
fn reports_missing_variables_and_invalid_placeholders_without_values() {
    let request = HttpRequest {
        headers: vec![("Authorization".into(), "private-prefix {{missing}}".into())],
        ..HttpRequest::default()
    };
    let error = resolve_variables(&request, &variables()).unwrap_err();

    assert_eq!(
        error,
        VariableError::Undefined {
            name: "missing".into(),
            field: "header value"
        }
    );
    assert!(!error.to_string().contains("private-prefix"));

    for path in ["{{unclosed", "{{}}", "{{ outer {{inner}} }}"] {
        let request = HttpRequest {
            path: path.into(),
            ..HttpRequest::default()
        };

        assert_eq!(
            resolve_variables(&request, &variables()).unwrap_err(),
            VariableError::InvalidPlaceholder { field: "URL" }
        );
    }
}

#[test]
fn keeps_binary_bodies_and_substituted_values_literal() {
    let mut variables = variables();
    variables.insert("token".into(), "{{literal}}".into());
    let template = HttpRequest {
        body: Some(b"\xff{{token}}".to_vec()),
        headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        ..HttpRequest::default()
    };
    let resolved = resolve_variables(&template, &variables).unwrap();

    assert_eq!(resolved.body, template.body);
    assert_eq!(resolved.headers[0].1, "Bearer {{literal}}");
}
