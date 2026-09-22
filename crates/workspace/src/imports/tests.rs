use request::{ApiKeyLocation, Authentication, Method};
use serde_json::json;

use super::parse_import;

#[test]
fn curl_preserves_quoted_json_headers_and_explicit_method() {
    let imported = parse_import(
        "curl --request=PATCH 'https://example.test/users?all=true&limit=2' \\\n         -H 'Content-Type: application/json' -H 'X-Tag: first' -H 'X-Tag: second' \\\n         --data-raw '{\"name\":\"Sam O'\"'\"'Neil\",\"cost\":\"$10\"}'",
    ).unwrap();
    let request = &imported[0].request;

    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.path, "https://example.test/users?all=true&limit=2");
    assert_eq!(
        request.body.as_deref(),
        Some(br#"{"name":"Sam O'Neil","cost":"$10"}"#.as_slice())
    );
    assert_eq!(
        request.headers,
        vec![
            ("Content-Type".into(), "application/json".into()),
            ("X-Tag".into(), "first".into()),
            ("X-Tag".into(), "second".into()),
        ]
    );
}

#[test]
fn curl_infers_post_encodes_form_values_and_preserves_basic_credentials() {
    let imported = parse_import(
        "curl --url https://example.test -u 'name:p:a:ss' --data-urlencode 'q=a b&c' -dcount=2",
    )
    .unwrap();
    let request = &imported[0].request;

    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.body.as_deref(),
        Some(b"q=a+b%26c&count=2".as_slice())
    );
    assert!(
        matches!(&request.authentication, Authentication::Basic { username, password } if username == "name" && password == "p:a:ss")
    );
    assert_eq!(
        request.headers,
        vec![(
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into()
        )]
    );
}

#[test]
fn curl_get_appends_data_before_fragment_and_head_remains_head() {
    let imported =
        parse_import("curl -G 'https://example.test/?x=1#details' --data-urlencode 'q=two words'")
            .unwrap();
    let request = &imported[0].request;

    assert_eq!(request.method, Method::Get);
    assert_eq!(
        request.path,
        "https://example.test/?x=1&q=two+words#details"
    );
    assert!(request.body.is_none());
    assert_eq!(
        parse_import("curl --head https://example.test").unwrap()[0]
            .request
            .method,
        Method::Head
    );
    assert_eq!(
        parse_import("curl -XOPTIONS https://example.test").unwrap()[0]
            .request
            .method,
        Method::Options
    );
}

#[test]
fn curl_json_shortcut_does_not_override_explicit_headers() {
    let imported = parse_import("curl https://example.test --compressed --json '{}' -H 'content-type: application/vnd.api+json' -H 'accept-encoding: identity'").unwrap();
    let request = &imported[0].request;

    assert_eq!(request.body.as_deref(), Some(b"{}".as_slice()));
    assert_eq!(
        request.headers,
        vec![
            ("content-type".into(), "application/vnd.api+json".into()),
            ("accept-encoding".into(), "identity".into()),
            ("Accept".into(), "application/json".into()),
        ]
    );
}

#[test]
fn curl_accepts_empty_and_literal_at_bodies_without_file_access() {
    let imported = parse_import("curl https://example.test --data-raw '' -H 'X-Empty;'").unwrap();

    assert_eq!(imported[0].request.body.as_deref(), Some(b"".as_slice()));
    assert_eq!(
        imported[0].request.headers[0],
        ("X-Empty".into(), String::new())
    );
    assert_eq!(
        parse_import("curl https://example.test --data-raw '@secret-file'").unwrap()[0]
            .request
            .body
            .as_deref(),
        Some(b"@secret-file".as_slice())
    );
}

#[test]
fn curl_rejects_shell_execution_file_access_and_semantic_loss() {
    for command in [
        "curl https://example.test; touch /tmp/never",
        "curl $(cat /tmp/never)",
        "curl \"https://example.test/$TOKEN\"",
        "curl `hostname`",
        "curl 'https://example.test",
        "curl https://example.test --data @private-file",
        "curl https://example.test --data-urlencode name@private-file",
        "curl https://example.test -H @private-file",
        "curl https://example.test -b private-file",
        "curl https://example.test --insecure",
        "curl https://example.test --location",
        "curl https://example.test --upload-file secret",
        "curl https://example.test https://another.test",
        "curl https://example.test -u username",
        "curl https://example.test --json '{}' -d a=b",
        "curl https://example.test -H 'Accept:'",
        "curl https://example.test -é",
        "curl https://example.test -X TRACE",
    ] {
        assert!(
            parse_import(command).is_err(),
            "Unexpectedly imported: {command}"
        );
    }
}

#[test]
fn curl_preserves_double_quote_backslashes_without_expanding() {
    let words =
        super::shell::words(r#"curl "https://example.test" --data-raw "one\ntwo\"three\$four""#)
            .unwrap();

    assert_eq!(words[3], "one\\ntwo\"three$four");
}

#[test]
fn postman_imports_nested_requests_with_query_once_and_disabled_fields_omitted() {
    let collection = json!({
        "info": {"name": "API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": [{"name": "People", "item": [{"name": "Update", "request": {
            "method": "PATCH",
            "url": {"raw": "https://example.test/users/:id?q=a%20b&skip=1", "query": [
                {"key": "q", "value": "a b"},
                {"key": "q", "value": "c"},
                {"key": "skip", "value": "1", "disabled": true}
            ], "variable": [{"key": "id", "value": "42"}]},
            "header": [{"key": "X-Enabled", "value": "yes"}, {"key": "X-Disabled", "value": "no", "disabled": true}],
            "body": {"mode": "raw", "raw": "{\"active\":true}", "options": {"raw": {"language": "json"}}}
        }}]}]
    });
    let imported = parse_import(&collection.to_string()).unwrap();
    let request = &imported[0].request;

    assert_eq!(imported[0].name, "Update");
    assert_eq!(imported[0].folders, ["People"]);
    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.path, "https://example.test/users/42");
    assert_eq!(
        request.query,
        Some(vec![("q".into(), "a b".into()), ("q".into(), "c".into())])
    );
    assert_eq!(
        request.headers,
        vec![
            ("X-Enabled".into(), "yes".into()),
            ("Content-Type".into(), "application/json".into())
        ]
    );
    assert_eq!(
        request.body.as_deref(),
        Some(br#"{"active":true}"#.as_slice())
    );
}

#[test]
fn postman_inherits_and_overrides_authentication() {
    let collection = json!({
        "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "collection-token"}]},
        "item": [
            {"name": "Inherited", "request": "https://example.test"},
            {"name": "Public", "request": {"url": "https://example.test", "auth": {"type": "noauth"}}},
            {"name": "Private", "auth": {"type": "basic", "basic": [{"key": "username", "value": "sam"}, {"key": "password", "value": "pass"}]}, "item": [
                {"name": "Inherited basic", "request": {"url": "https://example.test", "auth": null}},
                {"name": "API key", "request": {"url": "https://example.test", "auth": {"type": "apikey", "apikey": [
                    {"key": "key", "value": "api_key"}, {"key": "value", "value": "secret"}, {"key": "in", "value": "query"}
                ]}}}
            ]}
        ]
    });
    let imported = parse_import(&collection.to_string()).unwrap();

    assert!(
        matches!(&imported[0].request.authentication, Authentication::Bearer { token } if token == "collection-token")
    );
    assert!(matches!(
        imported[1].request.authentication,
        Authentication::None
    ));
    assert!(
        matches!(&imported[2].request.authentication, Authentication::Basic { username, password } if username == "sam" && password == "pass")
    );
    assert!(
        matches!(&imported[3].request.authentication, Authentication::ApiKey { name, value, location: ApiKeyLocation::Query } if name == "api_key" && value == "secret")
    );
}

#[test]
fn postman_builds_structured_urls_and_encodes_form_bodies() {
    let imported = parse_import(&json!({"item": [{"name": "Form", "request": {
        "method": "POST",
        "url": {"protocol": "http", "host": ["api", "example", "test"], "port": "8080", "path": ["v1", "login"]},
        "header": "Accept: application/json\nX-Extra: value",
        "body": {"mode": "urlencoded", "urlencoded": [{"key": "user", "value": "a b+c"}, {"key": "off", "disabled": true}]}
    }}]}).to_string()).unwrap();
    let request = &imported[0].request;

    assert_eq!(request.path, "http://api.example.test:8080/v1/login");
    assert_eq!(request.body.as_deref(), Some(b"user=a+b%2Bc".as_slice()));
    assert_eq!(
        request.headers[0],
        ("Accept".into(), "application/json".into())
    );
}

#[test]
fn postman_resolves_collection_defaults_and_preserves_environment_placeholders() {
    let imported = parse_import(
        &json!({
            "variable": [
                {"key": "host", "value": "https://example.test"},
                {"key": "base", "value": "{{host}}/api"},
                {"key": "count", "value": 3},
                {"key": "token", "value": "disabled", "disabled": true}
            ],
            "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]},
            "item": [{"name": "Defaults", "request": {
                "method": "POST", "url": "{{base}}/items",
                "header": [{"key": "X-Base", "value": "{{base}}"}],
                "body": {"mode": "raw", "raw": "{\"count\":{{count}}}"}
            }}]
        })
        .to_string(),
    )
    .unwrap();
    let request = &imported[0].request;

    assert_eq!(request.path, "https://example.test/api/items");
    assert_eq!(request.headers[0].1, "https://example.test/api");
    assert_eq!(request.body.as_deref(), Some(br#"{"count":3}"#.as_slice()));
    assert!(
        matches!(&request.authentication, Authentication::Bearer { token } if token == "{{token}}")
    );
}

#[test]
fn postman_reports_unsupported_data_before_returning_partial_import() {
    for request in [
        json!({"url": "https://example.test", "body": {"mode": "file", "file": {"src": "/private"}}}),
        json!({"url": "https://example.test", "body": {"mode": "formdata", "formdata": []}}),
        json!({"url": "https://example.test", "auth": {"type": "oauth2"}}),
        json!({"url": "https://example.test", "method": "TRACE"}),
        json!({"url": "https://example.test", "body": {"mode": "raw", "raw": {"invalid": "object"}}}),
    ] {
        let collection = json!({"item": [
            {"name": "Good", "request": "https://example.test"},
            {"name": "Unsupported", "request": request}
        ]});

        assert!(
            parse_import(&collection.to_string())
                .unwrap_err()
                .contains("Unsupported:")
        );
    }

    let scripts = json!({"item": [{"request": "https://example.test"}], "event": [{"listen": "prerequest", "script": {"exec": ["pm.variables.set('token', 'secret')"]}}]});
    assert!(
        parse_import(&scripts.to_string())
            .unwrap_err()
            .contains("scripts")
    );
}

#[test]
fn postman_reports_variable_cycles_and_empty_or_invalid_documents() {
    let cycle = json!({"variable": [{"key": "a", "value": "{{b}}"}, {"key": "b", "value": "{{a}}"}], "item": [{"request": "{{a}}"}]});
    assert!(
        parse_import(&cycle.to_string())
            .unwrap_err()
            .contains("cycle")
    );

    for input in [
        "",
        "{}",
        "{invalid}",
        "{\"item\":[]}",
        "[]",
        "wget https://example.test",
    ] {
        assert!(
            parse_import(input).is_err(),
            "Unexpectedly imported {input:?}"
        );
    }
}
