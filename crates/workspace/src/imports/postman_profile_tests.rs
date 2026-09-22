use request::{ApiKeyLocation, Authentication};
use serde_json::{Value, json};

use super::parse_import;

// Redirect, body-pruning and generated-header cases were verified with Postman
// Runtime 7.56.1 against loopback servers. TLS cases follow requester/core.js.
fn collection(profile: Value, request: Value) -> Value {
    json!({"protocolProfileBehavior": profile, "item": [{"request": request}]})
}

fn get() -> Value {
    json!({"url": "https://example.test/path"})
}

#[test]
fn postman_rejects_transport_profile_overrides_the_request_model_cannot_keep() {
    for (key, value) in [
        ("followRedirects", json!(false)),
        ("followRedirects", json!(null)),
        ("followOriginalHttpMethod", json!(true)),
        ("followAuthorizationHeader", json!(true)),
        ("removeRefererHeaderOnRedirect", json!(true)),
        ("disableUrlEncoding", json!(true)),
        ("maxRedirects", json!(1)),
        ("strictSSL", json!(false)),
        ("strictSSL", json!(true)),
        ("protocolVersion", json!("http2")),
        ("protocolVersion", json!("http1")),
        ("protocolVersion", json!(null)),
        ("insecureHTTPParser", json!(true)),
        ("insecureHTTPParser", json!(null)),
        ("tlsDisabledProtocols", json!(["TLSv1_2"])),
        ("tlsDisabledProtocols", json!(["TLSv1_3"])),
        ("tlsDisabledProtocols", json!(["TICKET"])),
        ("tlsCipherSelection", json!(["AES128-SHA"])),
        ("tlsCipherSelection", json!([null, null])),
    ] {
        let error = parse_import(&collection(json!({key: value}), get()).to_string()).unwrap_err();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn postman_keeps_known_inert_and_default_compatible_profile_metadata() {
    for profile in [
        json!({"followRedirects": true}),
        json!({"followRedirects": "false"}),
        json!({"followOriginalHttpMethod": false, "followAuthorizationHeader": null,
            "removeRefererHeaderOnRedirect": 0, "disableUrlEncoding": ""}),
        json!({"disableBodyPruning": true}),
        json!({"disableCookies": true}),
        json!({"insecureHTTPParser": false}),
        json!({"tlsPreferServerCiphers": true}),
        json!({"tlsDisabledProtocols": ["SSLv2", "SSLv3", "TLSv1", "TLSv1_1", "unknown"]}),
        json!({"tlsCipherSelection": []}),
        json!({"tlsCipherSelection": [null]}),
        json!({"tlsDisabledProtocols": "ignored", "tlsCipherSelection": "ignored"}),
        json!({"unrecognizedMetadata": true}),
    ] {
        assert!(
            parse_import(&collection(profile.clone(), get()).to_string()).is_ok(),
            "{profile}"
        );
    }
}

#[test]
fn postman_transport_profiles_inherit_shallowly_and_request_properties_are_ignored() {
    let source = json!({
        "protocolProfileBehavior": {"followRedirects": false},
        "item": [{"protocolProfileBehavior": {"followRedirects": true}, "item": [{"request": get()}]}]
    });
    assert!(parse_import(&source.to_string()).is_ok());

    let source = json!({
        "protocolProfileBehavior": {"followRedirects": false},
        "item": [{"protocolProfileBehavior": {"unrelated": true}, "request": get()}]
    });
    assert!(
        parse_import(&source.to_string())
            .unwrap_err()
            .contains("followRedirects")
    );

    let mut request = get();
    request["protocolProfileBehavior"] = json!({"followRedirects": false});
    assert!(parse_import(&collection(json!({}), request).to_string()).is_ok());

    let source = json!({"item": [{"protocolProfileBehavior": {"followRedirects": false},
        "request": "https://example.test"}]});
    assert!(
        parse_import(&source.to_string())
            .unwrap_err()
            .contains("followRedirects")
    );
}

#[test]
fn postman_rejects_suppressing_headers_that_the_client_would_generate() {
    for key in ["host", "accept", "accept-encoding", "content-length"] {
        let request = json!({"method": "POST", "url": "https://example.test/path"});
        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn postman_keeps_suppression_when_explicit_headers_or_request_shape_make_it_inert() {
    for key in [
        "host",
        "accept",
        "accept-encoding",
        "content-length",
        "connection",
    ] {
        let request = json!({"method": "POST", "url": "https://example.test/path",
            "header": [{"key": key, "value": "explicit"}]});
        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        assert!(parse_import(&source.to_string()).is_ok(), "{key}");
    }

    for profile in [
        json!({"disabledSystemHeaders": {"user-agent": true, "cache-control": true,
            "postman-token": true, "connection": true}}),
        json!({"disabledSystemHeaders": {"content-length": true}}),
        json!({"disabledSystemHeaders": {"Accept": true, "transfer-encoding": true}}),
    ] {
        assert!(parse_import(&collection(profile, get()).to_string()).is_ok());
    }

    let mut request = get();
    request["header"] = json!([{"key": "Range", "value": "bytes=0-10"}]);
    let source = collection(
        json!({"disabledSystemHeaders": {"accept-encoding": true}}),
        request,
    );
    assert!(parse_import(&source.to_string()).is_ok());
}

#[test]
fn postman_rejects_profile_suppression_that_removes_saved_system_headers() {
    for key in [
        "host",
        "accept-encoding",
        "connection",
        "content-length",
        "content-type",
    ] {
        let mut request = get();
        request["header"] = json!([{"key": key, "value": "system", "system": true}]);
        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains(key), "{error}");
    }

    let mut request = get();
    request["header"] = json!([{"key": "Transfer-Encoding", "value": "chunked", "system": true}]);
    let source = collection(
        json!({"disabledSystemHeaders": {"content-length": true}}),
        request,
    );
    assert!(parse_import(&source.to_string()).is_err());

    let mut request = get();
    request["header"] = json!([{"key": "Accept", "value": "*/*", "system": true}]);
    let source = collection(json!({"disabledSystemHeaders": {"accept": true}}), request);
    assert!(parse_import(&source.to_string()).is_ok());
}

#[test]
fn postman_rejects_suppression_that_removes_api_key_headers() {
    for (name, key, value) in [
        ("Host", "host", "custom.example.test"),
        ("Content-Type", "content-type", "application/custom"),
        ("cOnTeNt-LeNgTh", "content-length", "0"),
        ("Accept-Encoding", "accept-encoding", "identity"),
        ("Connection", "connection", "close"),
    ] {
        let mut request = get();
        request["header"] = json!([]);
        request["auth"] = json!({"type": "apikey", "apikey": {
            "key": name, "value": value, "in": "header"
        }});

        if key == "accept-encoding" {
            // Range prevents default Accept-Encoding generation, but Postman
            // still removes the system-owned header injected by the helper.
            request["header"] = json!([{"key": "Range", "value": "bytes=0-10"}]);
        }

        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.to_ascii_lowercase().contains(key), "{name}: {error}");
    }
}

#[test]
fn postman_length_suppression_removes_api_key_transfer_encoding_with_explicit_length() {
    let request = json!({
        "method": "POST", "url": "https://example.test/path",
        "header": [{"key": "Content-Length", "value": "0"}],
        "auth": {"type": "apikey", "apikey": {
            "key": "Transfer-Encoding", "value": "chunked", "in": "header"
        }}
    });
    let source = collection(
        json!({"disabledSystemHeaders": {"content-length": true}}),
        request,
    );
    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("content-length"), "{error}");
}

#[test]
fn postman_checks_suppressed_inherited_api_key_headers_on_string_requests() {
    let mut source = collection(
        json!({"disabledSystemHeaders": {"content-length": true}}),
        json!("https://example.test/path"),
    );
    source["auth"] = json!({"type": "apikey", "apikey": {
        "key": "Content-Length", "value": "0", "in": "header"
    }});

    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("content-length"), "{error}");

    source["protocolProfileBehavior"]["disabledSystemHeaders"]["content-length"] = json!(false);
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.authentication,
        Authentication::ApiKey {
            name: "Content-Length".into(),
            value: "0".into(),
            location: ApiKeyLocation::Header,
        }
    );
}

#[test]
fn postman_keeps_api_key_headers_that_suppression_does_not_remove() {
    for (name, key) in [
        ("Accept", "accept"),
        ("User-Agent", "user-agent"),
        ("Cache-Control", "cache-control"),
        ("Postman-Token", "postman-token"),
    ] {
        let mut request = get();
        request["auth"] = json!({"type": "apikey", "apikey": {
            "key": name, "value": "custom", "in": "header"
        }});

        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.authentication,
            Authentication::ApiKey {
                name: name.into(),
                value: "custom".into(),
                location: ApiKeyLocation::Header,
            }
        );
    }
}

#[test]
fn postman_rejects_variable_api_key_header_names_under_affected_suppression() {
    for key in [
        "host",
        "content-type",
        "content-length",
        "accept-encoding",
        "connection",
    ] {
        let mut request = get();
        request["auth"] = json!({"type": "apikey", "apikey": {
            "key": "{{header_name}}", "value": "custom", "in": "header"
        }});

        let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("API-key"), "{key}: {error}");
    }
}

#[test]
fn postman_keeps_variable_api_key_names_under_inert_or_ignored_suppression() {
    for profile in [
        json!({"disabledSystemHeaders": {"user-agent": true, "cache-control": true,
            "postman-token": true}}),
        json!({"disabledSystemHeaders": {"Content-Length": true,
            "transfer-encoding": true, "unknown": true}}),
        json!({"disabledSystemHeaders": {"host": false, "content-type": false,
            "content-length": false, "accept-encoding": false, "connection": false}}),
    ] {
        let mut request = get();
        request["auth"] = json!({"type": "apikey", "apikey": {
            "key": "{{header_name}}", "value": "custom", "in": "header"
        }});

        let source = collection(profile, request);
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.authentication,
            Authentication::ApiKey {
                name: "{{header_name}}".into(),
                value: "custom".into(),
                location: ApiKeyLocation::Header,
            }
        );
    }
}

#[test]
fn postman_query_api_keys_do_not_create_suppressed_system_headers() {
    for key in ["content-type", "content-length", "connection"] {
        for name in [key, "{{header_name}}"] {
            let mut request = get();
            request["auth"] = json!({"type": "apikey", "apikey": {
                "key": name, "value": "custom", "in": "query"
            }});

            let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
            let imported = parse_import(&source.to_string()).unwrap();
            assert_eq!(
                imported[0].request.authentication,
                Authentication::ApiKey {
                    name: name.into(),
                    value: "custom".into(),
                    location: ApiKeyLocation::Query,
                }
            );
        }
    }
}

#[test]
fn postman_range_header_helper_makes_accept_encoding_suppression_inert() {
    let mut request = get();
    request["auth"] = json!({"type": "apikey", "apikey": {
        "key": "Range", "value": "bytes=0-10", "in": "header"
    }});
    let profile = json!({"disabledSystemHeaders": {"accept-encoding": true}});
    let source = collection(profile.clone(), request.clone());
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.authentication,
        Authentication::ApiKey {
            name: "Range".into(),
            value: "bytes=0-10".into(),
            location: ApiKeyLocation::Header,
        }
    );

    request["auth"]["apikey"]["in"] = json!("query");
    let source = collection(profile, request);
    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("accept-encoding"), "{error}");
}

#[test]
fn postman_rejects_variable_system_header_names_under_affected_suppression() {
    for key in [
        "content-length",
        "content-type",
        "host",
        "accept-encoding",
        "connection",
    ] {
        for system in [json!(true), json!("false")] {
            let mut request = get();
            request["header"] = json!([
                {"key": key, "value": "explicit"},
                {"key": "{{header_name}}", "value": "0", "system": system}
            ]);

            let source = collection(json!({"disabledSystemHeaders": {key: true}}), request);
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(
                error.contains("variable system-owned header name"),
                "{key}: {error}"
            );
        }
    }
}

#[test]
fn postman_keeps_variable_headers_without_active_system_ownership() {
    for header in [
        json!({"key": "{{header_name}}", "value": "custom"}),
        json!({"key": "{{header_name}}", "value": "custom", "system": false}),
        json!({"key": "{{header_name}}", "value": "custom", "system": null}),
        json!({"key": "{{header_name}}", "value": "custom", "system": true, "disabled": true}),
    ] {
        let mut request = get();
        request["header"] = json!([
            {"key": "Host", "value": "example.test"},
            {"key": "Content-Type", "value": "application/custom"},
            {"key": "Content-Length", "value": "0"},
            {"key": "Accept-Encoding", "value": "identity"},
            {"key": "Connection", "value": "close"},
            header
        ]);
        let source = collection(
            json!({"disabledSystemHeaders": {"host": true, "content-type": true,
                "content-length": true, "accept-encoding": true, "connection": true}}),
            request,
        );
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0]
                .request
                .headers
                .iter()
                .any(|(name, _)| name == "{{header_name}}"),
            header.get("disabled").and_then(Value::as_bool) != Some(true)
        );
    }
}

#[test]
fn postman_keeps_variable_system_headers_under_inert_or_ignored_suppression() {
    for profile in [
        json!({"disabledSystemHeaders": {"accept": true, "user-agent": true,
            "cache-control": true, "postman-token": true}}),
        json!({"disabledSystemHeaders": {"Content-Length": true,
            "transfer-encoding": true, "unknown": true}}),
        json!({"disabledSystemHeaders": {"host": false, "content-type": false,
            "content-length": false, "accept-encoding": false, "connection": false}}),
    ] {
        let mut request = get();
        request["header"] = json!([
            {"key": "Accept", "value": "*/*"},
            {"key": "{{header_name}}", "value": "custom", "system": true}
        ]);

        let source = collection(profile, request);
        let imported = parse_import(&source.to_string()).unwrap();
        assert!(
            imported[0]
                .request
                .headers
                .iter()
                .any(|(name, _)| name == "{{header_name}}")
        );
    }
}

#[test]
fn postman_checks_system_header_names_after_resolving_collection_defaults() {
    let mut request = get();
    request["header"] = json!([
        {"key": "{{header_name}}", "value": "0", "system": true}
    ]);
    let mut source = collection(
        json!({"disabledSystemHeaders": {"content-length": true}}),
        request,
    );
    source["variable"] = json!([{"key": "header_name", "value": "X-Custom"}]);
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.headers,
        [("X-Custom".into(), "0".into())]
    );

    source["variable"][0]["value"] = json!("Transfer-Encoding");
    let error = parse_import(&source.to_string()).unwrap_err();
    assert!(error.contains("content-length"), "{error}");
}
