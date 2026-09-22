use serde_json::{Value, json};

use super::parse_import;

// These cases were checked against Postman Runtime 7.56.1 and its
// postman-request 2.88.1-postman.49 form encoder using a local HTTP server.
fn request(mode: &str, content_type: Option<&str>) -> Value {
    let mut request = json!({
        "method": "POST", "url": "https://example.test/submit",
        "body": {"mode": mode, mode: [{"key": "message", "value": "hello", "type": "text"}]}
    });

    if let Some(content_type) = content_type {
        request["header"] = json!([{"key": "Content-Type", "value": content_type}]);
    }

    request
}

#[test]
fn postman_forms_reject_parameters_that_native_runtime_preserves() {
    for (mode, content_type) in [
        (
            "urlencoded",
            "application/x-www-form-urlencoded; charset=UTF-8",
        ),
        (
            "urlencoded",
            "application/x-www-form-urlencoded; profile=vendor",
        ),
        ("urlencoded", "application/x-www-form-urlencoded-extra"),
        ("formdata", "multipart/form-data; boundary=custom"),
        (
            "formdata",
            "multipart/form-data;boundary=\"custom\"; profile=vendor",
        ),
        ("formdata", "multipart/form-data; boundary=\"\""),
        (
            "formdata",
            "multipart/form-data; boundary=one; boundary=two",
        ),
        ("urlencoded", "{{content_type}}"),
        ("formdata", "{{content_type}}"),
    ] {
        let source = json!({"item": [{"request": request(mode, Some(content_type))}]});
        assert!(
            parse_import(&source.to_string()).is_err(),
            "Unexpectedly imported {mode}: {content_type}"
        );
    }
}

#[test]
fn postman_forms_keep_bare_headers_and_custom_types_that_runtime_replaces() {
    for (mode, content_type) in [
        ("urlencoded", None),
        ("formdata", None),
        ("urlencoded", Some("application/x-www-form-urlencoded")),
        ("formdata", Some("multipart/form-data")),
        ("urlencoded", Some("application/vnd.example.form")),
        ("formdata", Some("application/vnd.example.form")),
        (
            "urlencoded",
            Some("Application/x-www-form-urlencoded; charset=UTF-8"),
        ),
        (
            "urlencoded",
            Some(" application/x-www-form-urlencoded; charset=UTF-8"),
        ),
        ("urlencoded", Some("application/x-www-form-urlencodedX")),
        ("urlencoded", Some("application/x-www-form-urlencoded; ")),
        ("formdata", Some("multipart/form-data; charset=UTF-8")),
        ("formdata", Some("multipart/form-data ; boundary=custom")),
        ("formdata", Some("multipart/form-data; BOUNDARY=custom")),
        ("formdata", Some("multipart/form-data; boundary=")),
    ] {
        let source = json!({"item": [{"request": request(mode, content_type)}]});
        assert!(
            parse_import(&source.to_string()).is_ok(),
            "Unexpectedly rejected {mode}: {content_type:?}"
        );
    }
}

#[test]
fn postman_form_system_header_override_inherits_only_from_collection_folders_and_item() {
    let profile = json!({"disabledSystemHeaders": {"content-type": true}});

    for mode in ["urlencoded", "formdata"] {
        for header in [None, Some("application/vnd.example.form")] {
            let request = request(mode, header);
            for source in [
                json!({"protocolProfileBehavior": profile, "item": [{"request": request}]}),
                json!({"item": [{"protocolProfileBehavior": profile, "item": [{"request": request}]}]}),
                json!({"item": [{"protocolProfileBehavior": profile, "request": request}]}),
            ] {
                let error = parse_import(&source.to_string()).unwrap_err();
                assert!(error.contains("disabled system Content-Type"));
            }

            let mut request = request;
            request["protocolProfileBehavior"] = profile.clone();
            assert!(parse_import(&json!({"item": [{"request": request}]}).to_string()).is_ok());
        }
    }
}

#[test]
fn postman_child_system_header_map_replaces_inherited_content_type_override() {
    for child_profile in [
        json!({"disabledSystemHeaders": {"content-type": false}}),
        json!({"disabledSystemHeaders": {}}),
        json!({"disabledSystemHeaders": {"user-agent": true}}),
    ] {
        let source = json!({
            "protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}},
            "item": [{"protocolProfileBehavior": child_profile, "item": [
                {"request": request("urlencoded", Some("application/vnd.example.form"))},
                {"request": request("formdata", Some("application/vnd.example.form"))}
            ]}]
        });
        assert!(parse_import(&source.to_string()).is_ok());
    }

    let inherited = json!({
        "protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}},
        "item": [{"protocolProfileBehavior": {"followRedirects": false}, "request": request("urlencoded", None)}]
    });
    assert!(parse_import(&inherited.to_string()).is_err());
}

#[test]
fn postman_content_type_override_does_not_reject_raw_disabled_or_unchanged_forms() {
    let profile = json!({"disabledSystemHeaders": {"content-type": true}});
    let mut raw = request("urlencoded", Some("application/vnd.example.raw"));
    raw["body"] = json!({"mode": "raw", "raw": "literal"});
    let mut disabled = request("urlencoded", Some("application/vnd.example.form"));
    disabled["body"]["disabled"] = json!(true);

    for request in [
        raw,
        disabled,
        request("urlencoded", Some("application/x-www-form-urlencoded")),
    ] {
        assert!(
            parse_import(
                &json!({"item": [{"protocolProfileBehavior": profile, "request": request}]})
                    .to_string()
            )
            .is_ok()
        );
    }
}

#[test]
fn postman_raw_bodies_reject_inherited_content_type_suppression_without_an_explicit_header() {
    let profile = json!({"disabledSystemHeaders": {"content-type": true}});

    for raw in ["hello", ""] {
        for headers in [
            json!([]),
            json!([{"key": "Accept", "value": "application/json"}]),
            json!([{"key": "Content-Type", "value": "text/plain", "disabled": true}]),
        ] {
            let request = json!({
                "method": "POST", "url": "https://example.test/submit", "header": headers,
                "body": {"mode": "raw", "raw": raw}
            });

            for source in [
                json!({"protocolProfileBehavior": profile, "item": [{"request": request}]}),
                json!({"item": [{"protocolProfileBehavior": profile, "item": [{"request": request}]}]}),
                json!({"item": [{"protocolProfileBehavior": profile, "request": request}]}),
            ] {
                let error = parse_import(&source.to_string()).unwrap_err();
                assert!(error.contains("enabled explicit Content-Type"), "{error}");
            }
        }
    }
}

#[test]
fn postman_raw_body_suppression_keeps_enabled_explicit_headers_and_skips_disabled_bodies() {
    let profile = json!({"disabledSystemHeaders": {"content-type": true}});

    for value in ["application/custom", ""] {
        for headers in [
            json!([{"key": "content-TYPE", "value": value}]),
            json!(format!("content-TYPE: {value}")),
        ] {
            let source = json!({"protocolProfileBehavior": profile, "item": [{"request": {
                "method": "POST", "url": "https://example.test/submit", "header": headers,
                "body": {"mode": "raw", "raw": "hello"}
            }}]});
            let imported = parse_import(&source.to_string()).unwrap();
            assert_eq!(
                imported[0].request.headers,
                [("content-TYPE".into(), value.into())]
            );
        }
    }

    for body in [
        json!(null),
        json!({"mode": "raw", "raw": "hello", "disabled": true}),
    ] {
        let source = json!({"protocolProfileBehavior": profile, "item": [{"request": {
            "method": "POST", "url": "https://example.test/submit", "body": body
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert!(imported[0].request.body.is_none());
        assert!(imported[0].request.headers.is_empty());
    }
}

#[test]
fn postman_raw_body_suppression_uses_the_last_enabled_headers_system_marker() {
    let ordinary = json!({"key": "Content-Type", "value": "application/custom"});
    let system = json!({"key": "Content-Type", "value": "text/plain", "system": true});
    let disabled = json!({"key": "Content-Type", "value": "text/plain", "disabled": true});

    for (headers, accepted) in [
        (json!([ordinary, system]), false),
        (json!([system, ordinary]), true),
        (json!([ordinary, disabled]), true),
        (json!([system, disabled]), false),
    ] {
        let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}}, "item": [{"request": {
            "method": "POST", "url": "https://example.test/submit", "header": headers,
            "body": {"mode": "raw", "raw": "hello"}
        }}]});
        assert_eq!(parse_import(&source.to_string()).is_ok(), accepted);
    }
}

#[test]
fn postman_empty_and_all_disabled_forms_send_no_body_for_every_supported_method() {
    for mode in ["formdata", "urlencoded"] {
        for fields in [
            json!([]),
            json!([{"key": "unused", "value": "value", "type": "text", "disabled": true}]),
        ] {
            for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
                let source = json!({"protocolProfileBehavior": {"disableBodyPruning": true}, "item": [{"request": {
                    "method": method, "url": "https://example.test", "body": {"mode": mode, mode: fields}
                }}]});
                let imported = parse_import(&source.to_string()).unwrap();
                assert!(imported[0].request.body.is_none());
                assert!(imported[0].request.form.is_none());
                assert!(imported[0].request.headers.is_empty());
            }
        }
    }
}

#[test]
fn postman_pruned_forms_keep_explicit_headers_and_skip_form_content_type_overrides() {
    for mode in ["formdata", "urlencoded"] {
        for content_type in [
            "application/custom",
            "multipart/form-data; boundary=custom",
            "application/x-www-form-urlencoded; charset=UTF-8",
            "",
        ] {
            let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true, "content-length": true}}, "item": [{"request": {
                "method": "POST", "url": "https://example.test", "body": {"mode": mode, mode: []},
                "header": [{"key": "Content-Type", "value": content_type}, {"key": "Content-Length", "value": "5"}]
            }}]});
            let imported = parse_import(&source.to_string()).unwrap();
            assert!(imported[0].request.form.is_none());
            assert_eq!(
                imported[0].request.headers,
                [
                    ("Content-Type".into(), content_type.into()),
                    ("Content-Length".into(), "5".into())
                ]
            );
        }

        let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}}, "item": [{"request": {
            "method": "POST", "url": "https://example.test", "body": {"mode": mode, mode: []},
            "header": [{"key": "Content-Type", "value": "disabled", "disabled": true}]
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert!(imported[0].request.form.is_none());
        assert!(imported[0].request.headers.is_empty());
    }
}

#[test]
fn postman_enabled_form_fields_with_empty_names_or_values_are_not_pruned() {
    for mode in ["formdata", "urlencoded"] {
        for key in ["field", ""] {
            let source = json!({"item": [{"request": {
                "method": "POST", "url": "https://example.test",
                "body": {"mode": mode, mode: [{"key": key, "value": "", "type": "text"}]}
            }}]});
            let imported = parse_import(&source.to_string()).unwrap();
            assert!(imported[0].request.form.is_some());
        }

        let source = json!({"protocolProfileBehavior": {"disabledSystemHeaders": {"content-length": true}}, "item": [{"request": {
            "method": "POST", "url": "https://example.test", "body": {"mode": mode, mode: []}
        }}]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("disabledSystemHeaders.content-length"));
    }
}

#[test]
fn postman_validates_content_type_supplied_by_api_key_helpers() {
    for (mode, content_type, accepted) in [
        (
            "urlencoded",
            "application/x-www-form-urlencoded; charset=UTF-8",
            false,
        ),
        ("formdata", "multipart/form-data; boundary=custom", false),
        ("urlencoded", "{{content_type}}", false),
        ("formdata", "{{content_type}}", false),
        ("urlencoded", "application/x-www-form-urlencoded", true),
        ("formdata", "multipart/form-data", true),
        ("urlencoded", "application/custom", true),
        ("formdata", "application/custom", true),
    ] {
        let source = json!({"auth": {"type": "apikey", "apikey": {
            "key": "content-TYPE", "value": content_type, "in": "header"
        }}, "item": [{"request": request(mode, None)}]});
        assert_eq!(
            parse_import(&source.to_string()).is_ok(),
            accepted,
            "{mode}: {content_type}"
        );
    }

    let mut request = request("formdata", None);
    request["auth"] = json!({"type": "apikey", "apikey": {"key": "{{header_name}}", "value": "value", "in": "header"}});
    assert!(parse_import(&json!({"item": [{"request": request}]}).to_string()).is_err());
}

#[test]
fn postman_pruned_forms_keep_helper_content_type_unless_system_headers_are_disabled() {
    for mode in ["formdata", "urlencoded"] {
        let request = json!({"method": "POST", "url": "https://example.test", "body": {"mode": mode, mode: []}});
        for suppressed in [false, true] {
            let source = json!({
                "auth": {"type": "apikey", "apikey": {"key": "Content-Type", "value": "application/custom", "in": "header"}},
                "protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": suppressed}},
                "item": [{"request": request}]
            });
            assert_eq!(parse_import(&source.to_string()).is_ok(), !suppressed);
        }
    }

    for request in [
        json!("https://example.test"),
        request("urlencoded", None),
        request("formdata", None),
    ] {
        let source = json!({
            "auth": {"type": "apikey", "apikey": {"key": "Content-Type", "value": "application/x-www-form-urlencoded", "in": "header"}},
            "protocolProfileBehavior": {"disabledSystemHeaders": {"content-type": true}},
            "item": [{"request": request}]
        });
        assert!(parse_import(&source.to_string()).is_err());
    }
}
