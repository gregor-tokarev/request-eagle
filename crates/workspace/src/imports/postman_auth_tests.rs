use request::{ApiKeyLocation, Authentication};
use serde_json::{Value, json};

use super::parse_import;

fn source(auth: Value, headers: Value) -> Value {
    json!({"item": [{"request": {"url": "https://example.test", "auth": auth, "header": headers}}]})
}

fn api_key(name: &str, value: &str, location: &str) -> Value {
    json!({"type": "apikey", "apikey": {"key": name, "value": value, "in": location}})
}

#[test]
fn postman_rejects_active_auth_helpers_that_replace_explicit_authorization() {
    for auth in [
        json!({"type": "basic", "basic": {"username": "user", "password": "pass"}}),
        json!({"type": "basic", "basic": {"username": "", "password": ""}}),
        json!({"type": "bearer", "bearer": {"token": "new"}}),
        json!({"type": "bearer", "bearer": {"token": "{{token}}"}}),
    ] {
        for name in ["Authorization", "authorization", "{{header_name}}"] {
            let source = source(auth.clone(), json!([{"key": name, "value": "old"}]));
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(error.contains("authentication may replace"), "{error}");
        }
    }
}

#[test]
fn postman_rejects_api_key_header_aliases_and_unknown_names_that_could_collide() {
    for name in [
        "X-Api-Key",
        "x-api-key",
        "X_API_KEY",
        "xApiKey",
        "{{header}}",
    ] {
        let source = source(
            api_key("X-Api-Key", "new", "header"),
            json!([{"key": name, "value": "old"}]),
        );
        assert!(parse_import(&source.to_string()).is_err(), "{name}");
    }

    let source = source(
        api_key("{{key_name}}", "{{token}}", "header"),
        json!([{"key": "X-Trace", "value": "value"}]),
    );
    assert!(parse_import(&source.to_string()).is_err());
}

#[test]
fn postman_query_api_key_rejects_exact_and_decoded_aliases_but_keeps_distinct_case() {
    for query in [
        "key=old",
        "key=one&key=two",
        "%6Bey=old",
        "{{query_name}}=old",
    ] {
        let source = json!({"auth": api_key("key", "new", "query"), "item": [
            {"request": format!("https://example.test/?{query}")}
        ]});
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(error.contains("authentication may replace"), "{error}");
    }

    let source = json!({"auth": api_key("key", "{{token}}", "query"), "item": [
        {"request": "https://example.test/?KEY=old&other={{value}}"}
    ]});
    assert!(parse_import(&source.to_string()).is_ok());

    let source = json!({"item": [{"auth": api_key("key", "new", "query"), "item": [
        {"request": {"url": {"host": "example.test", "query": [{"key": "key", "value": "{{value}}"}]}}}
    ]}]});
    assert!(parse_import(&source.to_string()).is_err());
}

#[test]
fn postman_keeps_nonconflicting_auth_variables_and_omits_disabled_credentials() {
    for auth in [
        json!({"type": "basic", "basic": {"username": "{{user}}", "password": "{{pass}}"}}),
        json!({"type": "bearer", "bearer": {"token": "{{token}}"}}),
        api_key("X-Api-Key", "{{token}}", "header"),
    ] {
        let source = source(
            auth,
            json!([
                {"key": "X-Trace", "value": "{{value}}"},
                {"key": "Authorization", "value": "old", "disabled": true},
                {"key": "X-Api-Key", "value": "old", "disabled": true}
            ]),
        );
        assert!(parse_import(&source.to_string()).is_ok());
    }

    for location in ["header", "query"] {
        let source = source(api_key("{{name}}", "{{token}}", location), json!([]));
        assert!(parse_import(&source.to_string()).is_ok());
    }
}

#[test]
fn postman_normalizes_inactive_auth_helpers_without_losing_explicit_headers() {
    for auth in [
        json!({"type": "bearer", "bearer": {"token": ""}}),
        api_key("", "", "header"),
        api_key("", "", "query"),
    ] {
        let source = source(auth, json!([{"key": "Authorization", "value": "old"}]));
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(imported[0].request.authentication, Authentication::None);
        assert_eq!(
            imported[0].request.headers,
            [("Authorization".into(), "old".into())]
        );
    }

    let imported =
        parse_import(&source(api_key("key", "", "query"), json!([])).to_string()).unwrap();
    assert_eq!(
        imported[0].request.authentication,
        Authentication::ApiKey {
            name: "key".into(),
            value: "".into(),
            location: ApiKeyLocation::Query
        }
    );
    assert!(parse_import(&source(api_key("", "value", "query"), json!([])).to_string()).is_err());
}

#[test]
fn postman_rejects_structured_url_auth_before_raw_or_request_auth_can_hide_it() {
    // Native SDK treats structured URL auth as authoritative over raw, decodes
    // percent escapes once, and uses it unless an active helper/header overrides it.
    for url_auth in [
        json!({"user": "sam", "password": "pass"}),
        json!({"user": "sam%40name", "password": "p%3Aa"}),
        json!({"user": "sam+name", "password": "p@ss:word"}),
        json!({"user": "{{username}}", "password": "{{password}}"}),
    ] {
        for raw in [
            None,
            Some("http://example.test"),
            Some("http://other:credentials@example.test"),
        ] {
            for auth in [
                json!({"type": "noauth"}),
                json!({"type": "basic", "basic": {"username": "helper", "password": "secret"}}),
            ] {
                let mut url = json!({"protocol": "http", "host": "example.test", "auth": url_auth});
                if let Some(raw) = raw {
                    url["raw"] = json!(raw);
                }
                let source = json!({"item": [{"request": {"url": url, "auth": auth,
                    "header": [{"key": "Authorization", "value": "explicit"}]}}]});
                let error = parse_import(&source.to_string()).unwrap_err();
                assert!(error.contains("structured URL authentication"), "{error}");
            }
        }
    }

    let source = json!({"item": [{"request": {"url": {"protocol": "http", "host": "example.test", "auth": null}}}]});
    assert!(parse_import(&source.to_string()).is_ok());
}
