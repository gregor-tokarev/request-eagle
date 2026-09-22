use std::collections::HashMap;

use request::{
    ApiKeyLocation, Authentication, FormBody, HttpRequest, Method, MultipartField, VariableError,
    resolve_variables,
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
        ..HttpRequest::default()
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

#[test]
fn explicit_authorization_skips_unused_basic_and_bearer_variables() {
    let variables = HashMap::from([("header".into(), "aUtHoRiZaTiOn".into())]);

    for authentication in [
        Authentication::Basic {
            username: "{{missing_username}}".into(),
            password: "{{missing_password}}".into(),
        },
        Authentication::Bearer {
            token: "{{missing_token}}".into(),
        },
    ] {
        let request = HttpRequest {
            headers: vec![("{{header}}".into(), "Bearer explicit-token".into())],
            authentication,
            ..HttpRequest::default()
        };
        let resolved = resolve_variables(&request, &variables).unwrap();

        assert_eq!(resolved.headers[0].1, "Bearer explicit-token");
        assert_eq!(resolved.authentication, request.authentication);

        let active_auth = HttpRequest {
            headers: Vec::new(),
            ..request
        };
        assert!(matches!(
            resolve_variables(&active_auth, &variables),
            Err(VariableError::Undefined { .. })
        ));
    }
}

#[test]
fn explicit_api_key_headers_and_queries_skip_unused_value_variables() {
    let variables = HashMap::from([("key".into(), "api_key".into())]);

    for request in [
        HttpRequest {
            headers: vec![("API_KEY".into(), "explicit-value".into())],
            authentication: Authentication::ApiKey {
                name: "{{key}}".into(),
                value: "{{missing_token}}".into(),
                location: ApiKeyLocation::Header,
            },
            ..HttpRequest::default()
        },
        HttpRequest {
            path: "https://example.test/path?api%5Fkey=explicit-value#fragment".into(),
            authentication: Authentication::ApiKey {
                name: "{{key}}".into(),
                value: "{{missing_token}}".into(),
                location: ApiKeyLocation::Query,
            },
            ..HttpRequest::default()
        },
        HttpRequest {
            path: "example.test/path?api_key=explicit-value".into(),
            authentication: Authentication::ApiKey {
                name: "{{key}}".into(),
                value: "{{missing_token}}".into(),
                location: ApiKeyLocation::Query,
            },
            ..HttpRequest::default()
        },
        HttpRequest {
            query: Some(vec![("{{key}}".into(), "explicit-value".into())]),
            authentication: Authentication::ApiKey {
                name: "{{key}}".into(),
                value: "{{missing_token}}".into(),
                location: ApiKeyLocation::Query,
            },
            ..HttpRequest::default()
        },
    ] {
        let resolved = resolve_variables(&request, &variables).unwrap();
        let Authentication::ApiKey { name, value, .. } = resolved.authentication else {
            panic!("preserve the configured authentication type");
        };

        assert_eq!(name, "api_key");
        assert_eq!(value, "{{missing_token}}");
    }

    for path in [
        "https://example.test/path?API_KEY=different-case",
        "https://example.test/path#fragment?api_key=not-a-query",
    ] {
        let request = HttpRequest {
            path: path.into(),
            authentication: Authentication::ApiKey {
                name: "{{key}}".into(),
                value: "{{missing_token}}".into(),
                location: ApiKeyLocation::Query,
            },
            ..HttpRequest::default()
        };

        assert_eq!(
            resolve_variables(&request, &variables).unwrap_err(),
            VariableError::Undefined {
                name: "missing_token".into(),
                field: "API key value",
            }
        );
    }
}

#[test]
fn form_owned_headers_skip_unused_api_key_value_variables() {
    let variables = HashMap::from([
        ("header".into(), "content-type".into()),
        ("length_header".into(), "content-length".into()),
        ("encoding_header".into(), "transfer-encoding".into()),
    ]);

    for form in [
        FormBody::UrlEncoded(vec![("name".into(), "eagle".into())]),
        FormBody::Multipart(vec![MultipartField::Text {
            name: "name".into(),
            value: "eagle".into(),
        }]),
    ] {
        for (name, expected_name) in [
            ("cOnTeNt-TyPe", "cOnTeNt-TyPe"),
            ("{{header}}", "content-type"),
            ("cOnTeNt-LeNgTh", "cOnTeNt-LeNgTh"),
            ("{{length_header}}", "content-length"),
            ("tRaNsFeR-EnCoDiNg", "tRaNsFeR-EnCoDiNg"),
            ("{{encoding_header}}", "transfer-encoding"),
        ] {
            let template = HttpRequest {
                method: Method::Post,
                form: Some(form.clone()),
                authentication: Authentication::ApiKey {
                    name: name.into(),
                    value: "{{unused_type}}".into(),
                    location: ApiKeyLocation::Header,
                },
                ..HttpRequest::default()
            };
            let resolved = resolve_variables(&template, &variables).unwrap();

            assert_eq!(
                resolved.authentication,
                Authentication::ApiKey {
                    name: expected_name.into(),
                    value: "{{unused_type}}".into(),
                    location: ApiKeyLocation::Header,
                }
            );
            assert_eq!(resolved.form, template.form);
            assert!(resolved.headers.is_empty());
            assert_eq!(
                template.authentication,
                Authentication::ApiKey {
                    name: name.into(),
                    value: "{{unused_type}}".into(),
                    location: ApiKeyLocation::Header,
                }
            );
        }
    }
}

#[test]
fn active_api_keys_still_require_values_when_forms_do_not_override_them() {
    let forms = [
        FormBody::UrlEncoded(vec![("name".into(), "eagle".into())]),
        FormBody::Multipart(vec![MultipartField::Text {
            name: "name".into(),
            value: "eagle".into(),
        }]),
    ];

    for (form, name, location) in [
        (None, "Content-Type", ApiKeyLocation::Header),
        (None, "Content-Length", ApiKeyLocation::Header),
        (None, "Transfer-Encoding", ApiKeyLocation::Header),
        (
            Some(forms[0].clone()),
            "Content-Type",
            ApiKeyLocation::Query,
        ),
        (
            Some(forms[1].clone()),
            "Content-Type",
            ApiKeyLocation::Query,
        ),
        (Some(forms[0].clone()), "X-Api-Key", ApiKeyLocation::Header),
        (Some(forms[1].clone()), "X-Api-Key", ApiKeyLocation::Header),
        (
            Some(forms[0].clone()),
            "Content-Length",
            ApiKeyLocation::Query,
        ),
        (
            Some(forms[1].clone()),
            "Transfer-Encoding",
            ApiKeyLocation::Query,
        ),
    ] {
        let template = HttpRequest {
            method: Method::Post,
            form,
            authentication: Authentication::ApiKey {
                name: name.into(),
                value: "{{unused_type}}".into(),
                location,
            },
            ..HttpRequest::default()
        };

        assert_eq!(
            resolve_variables(&template, &HashMap::new()).unwrap_err(),
            VariableError::Undefined {
                name: "unused_type".into(),
                field: "API key value",
            }
        );
    }

    for name in ["Content-Type", "Content-Length", "Transfer-Encoding"] {
        let raw_body = HttpRequest {
            method: Method::Post,
            body: Some(b"{}".to_vec()),
            authentication: Authentication::ApiKey {
                name: name.into(),
                value: "{{unused_type}}".into(),
                location: ApiKeyLocation::Header,
            },
            ..HttpRequest::default()
        };

        assert_eq!(
            resolve_variables(&raw_body, &HashMap::new()).unwrap_err(),
            VariableError::Undefined {
                name: "unused_type".into(),
                field: "API key value",
            }
        );
    }
}

#[test]
fn resolves_form_fields_and_upload_paths_without_changing_templates() {
    let variables = HashMap::from([
        ("name".into(), "user name".into()),
        ("value".into(), "eagle & bird".into()),
        ("file_field".into(), "attachment".into()),
        ("file_path".into(), "/tmp/example upload.bin".into()),
    ]);

    for (form, expected) in [
        (
            FormBody::UrlEncoded(vec![("{{name}}".into(), "{{value}}".into())]),
            FormBody::UrlEncoded(vec![("user name".into(), "eagle & bird".into())]),
        ),
        (
            FormBody::Multipart(vec![
                MultipartField::Text {
                    name: "{{name}}".into(),
                    value: "{{value}}".into(),
                },
                MultipartField::File {
                    name: "{{file_field}}".into(),
                    path: "{{file_path}}".into(),
                },
            ]),
            FormBody::Multipart(vec![
                MultipartField::Text {
                    name: "user name".into(),
                    value: "eagle & bird".into(),
                },
                MultipartField::File {
                    name: "attachment".into(),
                    path: "/tmp/example upload.bin".into(),
                },
            ]),
        ),
    ] {
        let template = HttpRequest {
            method: Method::Patch,
            form: Some(form.clone()),
            body: Some(b"{{inactive_raw_body}}".to_vec()),
            ..HttpRequest::default()
        };
        let resolved = resolve_variables(&template, &variables).unwrap();

        assert_eq!(resolved.form, Some(expected));
        assert_eq!(template.form, Some(form));
        assert_eq!(resolved.body, template.body);
    }
}

#[test]
fn missing_upload_path_variables_report_the_field_before_opening_files() {
    let template = HttpRequest {
        method: Method::Patch,
        form: Some(FormBody::Multipart(vec![MultipartField::File {
            name: "attachment".into(),
            path: "{{missing_file}}".into(),
        }])),
        ..HttpRequest::default()
    };

    assert_eq!(
        resolve_variables(&template, &HashMap::new()).unwrap_err(),
        VariableError::Undefined {
            name: "missing_file".into(),
            field: "upload file path",
        }
    );
}
