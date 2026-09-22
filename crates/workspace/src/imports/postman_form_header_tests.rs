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
