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
