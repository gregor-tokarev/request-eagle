use request::{ApiKeyLocation, Authentication};
use serde_json::json;

use super::parse_import;

#[test]
fn postman_imports_object_and_array_authentication_attributes() {
    for (kind, attributes, expected) in [
        (
            "basic",
            json!({"username": "sam", "password": "pass"}),
            Authentication::Basic {
                username: "sam".into(),
                password: "pass".into(),
            },
        ),
        (
            "bearer",
            json!({"token": "secret"}),
            Authentication::Bearer {
                token: "secret".into(),
            },
        ),
        (
            "apikey",
            json!({"key": "X-Api-Key", "value": "secret", "in": "header"}),
            Authentication::ApiKey {
                name: "X-Api-Key".into(),
                value: "secret".into(),
                location: ApiKeyLocation::Header,
            },
        ),
        (
            "apikey",
            json!({"key": "api_key", "value": "secret", "in": "query"}),
            Authentication::ApiKey {
                name: "api_key".into(),
                value: "secret".into(),
                location: ApiKeyLocation::Query,
            },
        ),
    ] {
        let array = attributes
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| json!({"key": key, "value": value, "type": "string"}))
            .collect::<Vec<_>>();

        for attributes in [attributes, json!(array)] {
            let collection = json!({"item": [{"request": {
                "url": "https://example.test",
                "auth": {"type": kind, (kind): attributes}
            }}]});
            let imported = parse_import(&collection.to_string()).unwrap();

            assert_eq!(imported[0].request.authentication, expected);
        }
    }
}

#[test]
fn postman_inherits_v2_object_authentication_and_resolves_defaults() {
    let collection = json!({
        "info": {"schema": "https://schema.getpostman.com/json/collection/v2.0.0/collection.json"},
        "variable": [{"key": "user", "value": "sam"}],
        "auth": {"type": "basic", "basic": {"username": "{{user}}", "password": "{{password}}"}},
        "item": [
            {"request": "https://example.test"},
            {"auth": {"type": "bearer", "bearer": {"token": "folder-token"}}, "item": [
                {"request": {"url": "https://example.test", "auth": null}},
                {"request": {"url": "https://example.test", "auth": {"type": "noauth"}}}
            ]}
        ]
    });
    let imported = parse_import(&collection.to_string()).unwrap();

    assert_eq!(
        imported[0].request.authentication,
        Authentication::Basic {
            username: "sam".into(),
            password: "{{password}}".into(),
        }
    );
    assert_eq!(
        imported[1].request.authentication,
        Authentication::Bearer {
            token: "folder-token".into(),
        }
    );
    assert_eq!(imported[2].request.authentication, Authentication::None);
}

#[test]
fn postman_rejects_malformed_authentication_attributes() {
    for attributes in [
        json!(42),
        json!("username=sam"),
        json!({"username": 42}),
        json!({"password": {"secret": "pass"}}),
        json!([{"key": "username", "value": false}]),
        json!([{"key": 42, "value": "sam"}]),
        json!(["username"]),
    ] {
        let collection = json!({"item": [{"name": "Bad auth", "request": {
            "url": "https://example.test",
            "auth": {"type": "basic", "basic": attributes}
        }}]});
        let error = parse_import(&collection.to_string()).unwrap_err();

        assert!(
            error.contains("Bad auth: Postman basic authentication"),
            "{error}"
        );
        assert!(error.contains("must be"), "{error}");
    }
}

#[test]
fn postman_raw_bodies_default_to_plain_text_and_keep_known_languages() {
    for (language, expected) in [
        (None, "text/plain"),
        (Some("text"), "text/plain"),
        (Some("json"), "application/json"),
        (Some("javascript"), "application/javascript"),
        (Some("xml"), "application/xml"),
        (Some("html"), "text/html"),
    ] {
        let mut body = json!({"mode": "raw", "raw": "hello"});

        if let Some(language) = language {
            body["options"] = json!({"raw": {"language": language}});
        }

        let collection = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test", "body": body
        }}]});
        let imported = parse_import(&collection.to_string()).unwrap();

        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(b"hello".as_slice())
        );
        assert_eq!(
            imported[0].request.headers,
            vec![("Content-Type".into(), expected.into())]
        );
    }
}

#[test]
fn postman_raw_body_content_type_never_overrides_an_explicit_header() {
    for body in [
        json!({"mode": "raw", "raw": "hello"}),
        json!({"mode": "raw", "raw": "hello", "options": {"raw": {"language": "json"}}}),
    ] {
        let collection = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test", "body": body,
            "header": [{"key": "content-TYPE", "value": "application/custom"}]
        }}]});
        let imported = parse_import(&collection.to_string()).unwrap();

        assert_eq!(
            imported[0].request.headers,
            vec![("content-TYPE".into(), "application/custom".into())]
        );
    }
}

#[test]
fn postman_structured_urls_default_to_http() {
    for host in [json!("example.test"), json!(["example", "test"])] {
        let collection = json!({"item": [{"request": {"url": {
            "host": host, "port": "8080", "path": ["items"],
            "query": [{"key": "page", "value": "2"}]
        }}}]});
        let imported = parse_import(&collection.to_string()).unwrap();

        assert_eq!(
            imported[0].request.path,
            "http://example.test:8080/items?page=2"
        );
    }
}

#[test]
fn postman_text_urls_materialize_http_without_changing_explicit_protocols() {
    for (source, expected) in [
        ("example.test/items", "http://example.test/items"),
        ("localhost:8080/items", "http://localhost:8080/items"),
        ("[::1]:8080/items", "http://[::1]:8080/items"),
        (
            "example.test/items?next=https://other.test",
            "http://example.test/items?next=https://other.test",
        ),
        ("https://example.test/items", "https://example.test/items"),
        ("http://example.test/items", "http://example.test/items"),
        ("HTTPS://example.test/items", "HTTPS://example.test/items"),
        ("{{base_url}}/items", "{{base_url}}/items"),
        ("{{protocol}}://example.test", "{{protocol}}://example.test"),
        ("http{{s}}://example.test", "http{{s}}://example.test"),
        ("example.test/{{id}}", "http://example.test/{{id}}"),
    ] {
        for request in [
            json!(source),
            json!({"url": source}),
            json!({"url": {"raw": source}}),
        ] {
            let collection = json!({"item": [{"request": request}]});
            let imported = parse_import(&collection.to_string()).unwrap();

            assert_eq!(imported[0].request.path, expected);
        }
    }
}

#[test]
fn postman_url_defaults_apply_after_collection_variable_resolution() {
    for (base, expected) in [
        ("example.test", "http://example.test/items"),
        ("https://example.test", "https://example.test/items"),
    ] {
        let collection = json!({
            "variable": [{"key": "base_url", "value": base}],
            "item": [{"request": {"url": "{{base_url}}/items"}}]
        });
        let imported = parse_import(&collection.to_string()).unwrap();

        assert_eq!(imported[0].request.path, expected);
    }
}

#[test]
fn postman_rejects_empty_and_hostless_text_urls() {
    for source in ["", "  ", "/items", "//example.test/items"] {
        for request in [
            json!(source),
            json!({"url": source}),
            json!({"url": {"raw": source}}),
        ] {
            let collection = json!({"item": [{"request": request}]});

            assert!(parse_import(&collection.to_string()).is_err(), "{source:?}");
        }
    }
}

#[test]
fn postman_rejects_unsupported_explicit_protocols() {
    for protocol in ["ftp", "file", "ws", "gopher"] {
        let source = format!("{protocol}://example.test/items");

        for request in [
            json!(source),
            json!({"url": source}),
            json!({"url": {"raw": source}}),
            json!({"url": {"protocol": protocol, "host": "example.test", "path": ["items"]}}),
        ] {
            let collection = json!({"item": [{"request": request}]});
            let error = parse_import(&collection.to_string()).unwrap_err();

            assert!(error.contains("not supported"), "{error}");
            assert!(error.contains(protocol), "{error}");
        }
    }
}

#[test]
fn postman_structured_target_fields_override_stale_raw_urls() {
    // Postman SDK 5.3.1 and Runtime 7.56.1 use structured fields exclusively.
    for raw in [
        "https://stale.test/wrong?old=1",
        "http://target.test:9999/wrong",
        "http://target.test:8080/users?q=stale",
    ] {
        let source = json!({"item": [{"request": {"url": {
            "raw": raw, "protocol": "http", "host": ["target", "test"],
            "port": "8080", "path": ["users"], "query": [{"key": "q", "value": "live"}]
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.path,
            "http://target.test:8080/users?q=live"
        );
    }
}

#[test]
fn postman_structured_urls_never_inherit_missing_path_or_query_from_raw() {
    for query in [None, Some(json!([]))] {
        let mut url = json!({"raw": "https://stale.test/wrong?old=1", "host": "target.test"});
        if let Some(query) = query {
            url["query"] = query;
        }
        let source = json!({"item": [{"request": {"url": url}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.path, "http://target.test/");
    }
}

#[test]
fn postman_structured_host_variables_can_supply_the_protocol() {
    for (host, variables, expected) in [
        (
            "http://example.test:8080",
            json!([]),
            "http://example.test:8080/items",
        ),
        (
            "{{base_url}}",
            json!([{"key": "base_url", "value": "https://example.test"}]),
            "https://example.test/items",
        ),
        ("{{base_url}}", json!([]), "{{base_url}}/items"),
    ] {
        let source = json!({"variable": variables, "item": [{"request": {"url": {
            "raw": "https://stale.test/ignored", "host": [host], "path": ["items"]
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.path, expected);
    }
}

#[test]
fn postman_accepts_raw_only_convenience_but_rejects_hostless_structured_urls() {
    let source = json!({"item": [{"request": {"url": {"raw": "https://example.test/items?x=1"}}}]});
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(imported[0].request.path, "https://example.test/items?x=1");

    for partial in [
        json!({"protocol": "https"}),
        json!({"host": null}),
        json!({"host": []}),
        json!({"path": ["items"]}),
        json!({"query": [{"key": "x", "value": "1"}]}),
    ] {
        let mut url = partial;
        url["raw"] = json!("https://stale.test/items");
        let source = json!({"item": [{"request": {"url": url}}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(
            error.contains("structured Postman URL has no host"),
            "{error}"
        );
    }
}

#[test]
fn postman_path_variables_select_last_duplicate_even_when_disabled() {
    for definitions in [
        json!([{"key": "id", "value": "first"}, {"key": "id", "value": "last"}]),
        json!([{"key": "id", "value": "first"}, {"key": "id", "value": "last", "disabled": true}]),
        json!([{"key": "id", "value": "last", "disabled": true}]),
    ] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "path": ["users", ":id.json"], "variable": definitions
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.path,
            "http://example.test/users/last.json"
        );
    }
}

#[test]
fn postman_empty_path_variable_masks_earlier_value_without_falling_back() {
    for last in [
        json!({"key": "id", "value": ""}),
        json!({"key": "id", "value": null}),
        json!({"key": "id"}),
        json!({"key": "id", "value": "", "disabled": true}),
    ] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "path": [":id", ":id.json"],
            "variable": [{"key": "id", "value": "first"}, last]
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.path, "http://example.test/:id/:id.json");
    }
}

#[test]
fn postman_path_variable_precedence_preserves_name_case_and_extensions() {
    let source = json!({"item": [{"request": {"url": {
        "host": "example.test", "path": [":id", ":ID", ":id-name.tar.gz"],
        "variable": [{"key": "id", "value": "lower"}, {"key": "ID", "value": "upper"},
            {"key": "id-name", "value": "first"}, {"key": "id-name", "value": "last"}]
    }}}]});
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.path,
        "http://example.test/lower/upper/last.tar.gz"
    );
}

#[test]
fn postman_string_headers_trim_sdk_whitespace_but_array_headers_preserve_values() {
    for whitespace in ['\u{00a0}', '\u{2003}', '\u{202f}', '\u{3000}', '\u{feff}'] {
        let value = format!("{whitespace}credential{whitespace}");
        for (headers, expected) in [
            (
                json!(format!("X-Key: \t{value} \t")),
                "credential".to_owned(),
            ),
            (json!([{"key": "X-Key", "value": value}]), value.clone()),
        ] {
            let source =
                json!({"item": [{"request": {"url": "https://example.test", "header": headers}}]});
            let imported = parse_import(&source.to_string()).unwrap();
            assert_eq!(imported[0].request.headers, [("X-Key".into(), expected)]);
        }
    }

    let source = json!({"item": [{"request": {"url": "https://example.test", "header": "X-Key: \u{0085}credential\u{0085}"}}]});
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.headers,
        [("X-Key".into(), "\u{0085}credential\u{0085}".into())]
    );
}

#[test]
fn postman_method_normalization_remains_separate_from_curl_custom_method_spelling() {
    for (method, expected) in [("get", "GET"), ("DeLeTe", "DELETE"), ("pAtCh", "PATCH")] {
        let source =
            json!({"item": [{"request": {"method": method, "url": "https://example.test"}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.method.as_str(), expected);
    }
}

#[test]
fn postman_rejects_case_variant_header_duplicates_but_preserves_exact_duplicates() {
    for headers in [
        json!([{"key": "Authorization", "value": "Bearer first"},
            {"key": "authorization", "value": "Bearer second"}]),
        json!("Authorization: Bearer first\nauthorization: Bearer second"),
        json!([{"key": "X-Api-Key", "value": "first"},
            {"key": "X-API-KEY", "value": "second"}]),
    ] {
        let source = json!({"item": [{"request": {
            "url": "https://example.test", "header": headers
        }}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("different capitalization"), "{error}");
    }

    for headers in [
        json!([{"key": "Authorization", "value": "Bearer first"},
            {"key": "Authorization", "value": "Bearer second"}]),
        json!("Authorization: Bearer first\nAuthorization: Bearer second"),
    ] {
        let source = json!({"item": [{"request": {
            "url": "https://example.test", "header": headers
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.headers,
            [
                ("Authorization".into(), "Bearer first".into()),
                ("Authorization".into(), "Bearer second".into())
            ]
        );
    }
}

#[test]
fn postman_path_variables_reject_declared_coercions_and_non_string_types() {
    for variable in [
        json!({"key": "id", "type": "string", "value": null}),
        json!({"key": "id", "type": "string"}),
        json!({"key": "id", "type": "string", "value": 7}),
        json!({"key": "id", "type": "number", "value": "007"}),
        json!({"key": "id", "type": "number", "value": 7}),
        json!({"key": "id", "type": "boolean", "value": true}),
        json!({"key": "id", "type": null, "value": "7"}),
        json!({"key": "id", "type": "unknown", "value": "7"}),
    ] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "path": ["users", ":id"], "variable": [variable]
        }}}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(
            error.contains("path variables") && error.contains("string type"),
            "{error}"
        );
    }

    for value in [json!(7), json!(1.0), json!(0.000001), json!(true)] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "path": ["users", ":id"],
            "variable": [{"key": "id", "value": value}]
        }}}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("field values must be strings"), "{error}");
    }

    for variable in [
        json!({"key": "id", "value": "007"}),
        json!({"key": "id", "type": "string", "value": "007"}),
        json!({"key": "id", "type": "STRING", "value": "007"}),
    ] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "path": ["users", ":id.json"], "variable": [variable]
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.path,
            "http://example.test/users/007.json"
        );
    }
}

#[test]
fn postman_retained_raw_text_without_a_body_mode_remains_inactive() {
    for method in ["GET", "POST"] {
        for headers in [
            json!([]),
            json!([{"key": "Content-Type", "value": "application/custom"}]),
        ] {
            let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}},
            "item": [{"request": {"method": method, "url": "https://example.test",
                "header": headers, "body": {"raw": "hello", "options": {"raw": {"language": "json"}}}
            }}]});
            let imported = parse_import(&source.to_string()).unwrap();
            assert!(imported[0].request.body.is_none());
            assert!(imported[0].request.form.is_none());
            let expected_headers = if headers.as_array().unwrap().is_empty() {
                vec![]
            } else {
                vec![("Content-Type".into(), "application/custom".into())]
            };
            assert_eq!(imported[0].request.headers, expected_headers);
        }
    }
}

#[test]
fn postman_empty_raw_bodies_remain_inactive_and_keep_explicit_headers() {
    for method in ["GET", "POST", "HEAD"] {
        for suppressed in [false, true] {
            for headers in [
                json!([]),
                json!([{"key": "Content-Type", "value": "application/custom"}]),
            ] {
                for body in [
                    json!({"mode": "raw", "raw": ""}),
                    json!({"mode": "raw", "raw": "", "options": {"raw": {"language": "json"}}}),
                    json!({"mode": "raw", "raw": null}),
                    json!({"mode": "raw"}),
                ] {
                    let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": suppressed}},
                    "item": [{"request": {"method": method, "url": "https://example.test",
                        "header": headers, "body": body
                    }}]});
                    let imported = parse_import(&source.to_string()).unwrap();
                    let request = &imported[0].request;
                    assert!(request.body.is_none());
                    assert!(request.form.is_none());
                    let expected_headers = if headers.as_array().unwrap().is_empty() {
                        vec![]
                    } else {
                        vec![("Content-Type".into(), "application/custom".into())]
                    };
                    assert_eq!(request.headers, expected_headers);
                }
            }
        }
    }

    let source = json!({"item": [{"request": {
        "method": "POST", "url": "https://example.test", "body": {"mode": "raw", "raw": " "}
    }}]});
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(imported[0].request.body.as_deref(), Some(b" ".as_slice()));
    assert_eq!(
        imported[0].request.headers,
        [("Content-Type".into(), "text/plain".into())]
    );
}

#[test]
fn postman_rejects_all_explicit_non_null_upload_filename_overrides() {
    for filename in [
        json!(""),
        json!("custom.txt"),
        json!(false),
        json!(0),
        json!({}),
        json!([]),
    ] {
        let source = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test", "body": {
                "mode": "formdata", "formdata": [{"key": "upload", "type": "file",
                    "src": "/tmp/original.txt", "fileName": filename}]
            }
        }}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("filenames"), "{error}");
    }

    for field in [
        json!({"key": "upload", "type": "file", "src": "/tmp/original.txt"}),
        json!({"key": "upload", "type": "file", "src": "/tmp/original.txt", "fileName": null}),
    ] {
        let source = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "body": {"mode": "formdata", "formdata": [field]}
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert!(matches!(
            imported[0].request.form,
            Some(request::FormBody::Multipart(_))
        ));
    }
}

#[test]
fn postman_rejects_encoded_dot_segments_after_constructing_the_final_url() {
    for segment in ["%2e", ".%2E", "%2e.", "%2E%2e"] {
        for url in [
            json!(format!("https://example.test/a/{segment}/b")),
            json!({"raw": format!("https://example.test/a/{segment}/b")}),
            json!({"host": "example.test", "path": ["a", segment, "b"]}),
            json!({"host": "example.test", "path": ["a", ":segment", "b"],
                "variable": [{"key": "segment", "value": segment}]}),
            json!(format!("{{{{base_url}}}}/a/{segment}/b")),
        ] {
            let source = json!({"item": [{"request": {"url": url}}]});
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(error.contains("encoded dot segments"), "{error}");
        }
    }

    let source = json!({"variable": [{"key": "segment", "value": "%2e%2e"}],
        "item": [{"request": "https://example.test/a/{{segment}}/b"}]});
    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("encoded dot segments"), "{error}");
}

#[test]
fn postman_keeps_backslashes_and_escapes_that_do_not_change_source_execution() {
    for path in [
        "/a/%2ejson/b",
        "/a/%252e%252e/b",
        "/a?path=/%2e%2e/b",
        "/a#/%2e%2e/b",
    ] {
        let url = format!("https://example.test{path}");
        let source = json!({"item": [{"request": {"url": url}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.path, url);
    }

    // Native Postman and the executor both turn literal path backslashes into
    // separators. cURL preserves them and has its own explicit rejection.
    for url in [
        json!("https://example.test/a\\b"),
        json!({"protocol": "https", "host": "example.test", "path": ["a\\b"]}),
        json!({"protocol": "https", "host": "example.test", "path": "/a\\b"}),
    ] {
        let source = json!({"item": [{"request": {"url": url}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            url::Url::parse(&imported[0].request.path).unwrap().path(),
            "/a/b"
        );
    }
}

#[test]
fn postman_query_templates_keep_variable_names_opaque_during_literal_decoding() {
    for name in ["token+name", "token%2Fname", "token&=name"] {
        let placeholder = format!("{{{{{name}}}}}");
        let source = json!({"item": [{"request": {"url": {
            "protocol": "https", "host": "example.test", "path": ["x"],
            "query": [{"key": placeholder, "value": format!("prefix%2F{placeholder}+suffix")},
                {"key": "literal%2Fkey", "value": "a%2Fb+c"}]
        }}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        let template = &imported[0].request;
        assert_eq!(
            template.query,
            Some(vec![
                (placeholder.clone(), format!("prefix/{placeholder} suffix")),
                ("literal/key".into(), "a/b c".into())
            ])
        );

        let resolved = request::resolve_variables(
            template,
            &std::collections::HashMap::from([(name.into(), "a&b=c".into())]),
        )
        .unwrap();
        let mut url = url::Url::parse(&resolved.path).unwrap();
        url.query_pairs_mut()
            .extend_pairs(resolved.query.as_ref().unwrap());
        assert_eq!(
            url.as_str(),
            "https://example.test/x?a%26b%3Dc=prefix%2Fa%26b%3Dc+suffix&literal%2Fkey=a%2Fb+c"
        );
    }
}

#[test]
fn postman_query_template_decoding_keeps_literal_safety_guards() {
    for (name, value, expected_error) in [
        ("q", "%7B{{token+name}}", "percent-encoded braces"),
        ("q", "{{token+name}}%7D", "percent-encoded braces"),
        ("%7B{{key}}", "value", "percent-encoded braces"),
        ("q", "%FF{{token+name}}", "UTF-8"),
        ("{{key}}%FF", "value", "UTF-8"),
    ] {
        let source = json!({"item": [{"request": {"url": {
            "host": "example.test", "query": [{"key": name, "value": value}]
        }}}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains(expected_error), "{error}");
    }

    let source = json!({"item": [{"request": {"url": {
        "host": "example.test", "query": [{"key": "q", "value": "{{token+name}}"}, {"key": "flag"}]
    }}}]});
    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("valueless flags"), "{error}");
}

#[test]
fn postman_rejects_url_controls_before_trimming_or_executor_normalization() {
    for control in ['\t', '\r', '\n'] {
        for text in [
            format!("https://example.test/a{control}b"),
            format!("https://example.test/a?q=one{control}two"),
            format!("https://example.test/a?q=one{control}"),
        ] {
            for request in [
                json!(text),
                json!({"url": text}),
                json!({"url": {"raw": text}}),
            ] {
                let source = json!({"item": [{"request": request}]});
                let error = parse_import(&source.to_string()).unwrap_err();
                assert!(error.contains("literal TAB, CR, or LF"), "{error}");
            }
        }

        for url in [
            json!({"host": "example.test", "path": format!("/a{control}b")}),
            json!({"host": "example.test", "path": [format!("a{control}b")]}),
            json!({"host": "example.test", "path": [":id"],
                "variable": [{"key": "id", "value": format!("a{control}b")}]}),
        ] {
            let source = json!({"item": [{"request": {"url": url}}]});
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(error.contains("literal TAB, CR, or LF"), "{error}");
        }

        let source = json!({"variable": [{"key": "value", "value": format!("a{control}b")}],
            "item": [{"request": "https://example.test/{{value}}"}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("literal TAB, CR, or LF"), "{error}");
    }
}

#[test]
fn postman_keeps_control_escapes_and_safely_encoded_structured_query_fields() {
    for (control, escaped) in [('\t', "%09"), ('\r', "%0D"), ('\n', "%0A")] {
        let text = format!("https://example.test/a{escaped}b?q=one{escaped}two");
        let source = json!({"item": [{"request": {"url": text}}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.path, text);

        for templated in [false, true] {
            let mut fields = vec![json!({"key": "q", "value": format!("one{control}two")})];

            if templated {
                fields.push(json!({"key": "other", "value": "{{value}}"}));
            }

            let source = json!({"item": [{"request": {"url": {
                "protocol": "https", "host": "example.test", "query": fields
            }}}]});
            let imported = parse_import(&source.to_string()).unwrap();
            let resolved = request::resolve_variables(
                &imported[0].request,
                &std::collections::HashMap::from([("value".into(), "ok".into())]),
            )
            .unwrap();
            let mut url = url::Url::parse(&resolved.path).unwrap();

            if let Some(query) = resolved.query {
                url.query_pairs_mut().extend_pairs(query);
            }

            let suffix = if templated { "&other=ok" } else { "" };
            assert_eq!(
                url.as_str(),
                format!("https://example.test/?q=one{escaped}two{suffix}")
            );
        }
    }
}

#[test]
fn postman_raw_defaults_reject_unresolved_enabled_header_names() {
    for language in [None, Some("json"), Some("text")] {
        for headers in [
            json!([{"key": "{{header_name}}", "value": "application/xml"}]),
            json!("{{header_name}}: application/xml"),
        ] {
            let mut body = json!({"mode": "raw", "raw": "{}"});

            if let Some(language) = language {
                body["options"] = json!({"raw": {"language": language}});
            }

            let source = json!({"item": [{"request": {
                "method": "POST", "url": "https://example.test", "header": headers, "body": body
            }}]});
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(error.contains("Unresolved Postman header names"), "{error}");
        }
    }
}

#[test]
fn postman_raw_defaults_keep_literal_and_collection_resolved_content_type_names() {
    for name in ["Content-Type", "{{header_name}}"] {
        let source = json!({"variable": [{"key": "header_name", "value": "Content-Type"}], "item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": name, "value": "application/xml"}],
            "body": {"mode": "raw", "raw": "{}", "options": {"raw": {"language": "json"}}}
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.headers,
            [("Content-Type".into(), "application/xml".into())]
        );
    }

    for body in [
        json!({"mode": "raw", "raw": ""}),
        json!({"mode": "raw", "raw": "{}", "disabled": true}),
    ] {
        let source = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": "{{header_name}}", "value": "application/xml"}], "body": body
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert!(imported[0].request.body.is_none());
        assert_eq!(
            imported[0].request.headers,
            [("{{header_name}}".into(), "application/xml".into())]
        );
    }

    let source = json!({"item": [{"request": {
        "method": "POST", "url": "https://example.test",
        "header": [{"key": "{{header_name}}", "value": "application/xml", "disabled": true}],
        "body": {"mode": "raw", "raw": "{}", "options": {"raw": {"language": "json"}}}
    }}]});
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.headers,
        [("Content-Type".into(), "application/json".into())]
    );
}
