use request::{ApiKeyLocation, Authentication, FormBody, Method, MultipartField};
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
            "url": {"raw": "https://example.test/users/:id?q=a%20b&skip=1",
                "protocol": "https", "host": ["example", "test"], "path": ["users", ":id"], "query": [
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
    assert_eq!(request.path, "https://example.test/users/42?q=a%20b&q=c");
    assert_eq!(request.query, None);
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
    assert_eq!(
        request.form,
        Some(FormBody::UrlEncoded(vec![("user".into(), "a b+c".into())]))
    );
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
        json!({"url": "https://example.test", "body": {"mode": "graphql", "graphql": {}}}),
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

#[test]
fn curl_imports_editable_multipart_without_reading_uploads() {
    let imported = parse_import("curl https://example.test/upload -F 'description= hello ' -F 'upload=@/nonexistent/file.bin ' --form-string 'literal=@text;not-a-file'").unwrap();

    assert_eq!(imported[0].request.method, Method::Post);
    assert_eq!(
        imported[0].request.form,
        Some(FormBody::Multipart(vec![
            MultipartField::Text {
                name: "description".into(),
                value: "hello".into()
            },
            MultipartField::File {
                name: "upload".into(),
                path: "/nonexistent/file.bin".into()
            },
            MultipartField::Text {
                name: "literal".into(),
                value: "@text;not-a-file".into()
            },
        ]))
    );

    for command in [
        "curl https://example.test -F 'file=@relative.bin'",
        "curl https://example.test -F 'file=@/tmp/file;type=text/plain'",
        "curl https://example.test -F 'file=@/tmp/a,/tmp/b'",
        "curl https://example.test -F 'text=</tmp/a'",
        "curl https://example.test -F 'x=y' -d 'a=b'",
        "curl https://example.test -G -F 'x=y'",
        "curl https://example.test -F 'x=\"quoted\"'",
        "curl https://example.test -F 'x=(nested'",
    ] {
        assert!(
            parse_import(command).is_err(),
            "Unexpectedly imported {command}"
        );
    }
}

#[test]
fn curl_header_shortcuts_follow_precedence_and_json_get_has_json_headers() {
    let imported = parse_import("curl https://example.test -A first -A second -e https://first.test -e https://second.test -H 'user-agent: explicit' -G --json '{}'").unwrap();
    let request = &imported[0].request;

    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "https://example.test?{}");
    assert_eq!(
        request.headers,
        vec![
            ("user-agent".into(), "explicit".into()),
            ("Referer".into(), "https://second.test".into()),
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "application/json".into()),
        ]
    );
}

#[test]
fn postman_preserves_escaped_queries_plus_signs_and_valueless_flags() {
    let imported = parse_import(
        &json!({"item": [{"request": {"url": {
            "raw": "https://example.test/?old=discarded",
            "protocol": "https", "host": "example.test",
            "query": [
                {"key": "q", "value": "a%2Fb+c"},
                {"key": "a=b", "value": "one&two=three#four"},
                {"key": "flag", "value": null},
                {"key": "empty", "value": ""},
                {"key": "utf8", "value": "Привет"},
                {"key": "bytes", "value": "%FF%zz"},
                {"key": "disabled", "value": "no", "disabled": true}
            ]
        }}}]})
        .to_string(),
    )
    .unwrap();
    let request = &imported[0].request;

    assert_eq!(
        request.path,
        "https://example.test/?q=a%2Fb+c&a%3Db=one%26two=three%23four&flag&empty=&utf8=%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82&bytes=%FF%zz"
    );
    assert!(request.query.is_none());
    assert_eq!(
        url::Url::parse(&request.path).unwrap().as_str(),
        request.path
    );
}

#[test]
fn postman_resolves_path_variables_before_raw_query_and_fragment() {
    for suffix in ["?q=one", "#details", "?q=one#details"] {
        let imported = parse_import(
            &json!({"item": [{"request": {"url": {
                "raw": format!("https://example.test/users/:id{suffix}"),
                "variable": [{"key": "id", "value": "42"}]
            }}}]})
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            imported[0].request.path,
            format!("https://example.test/users/42{suffix}")
        );
    }
}

#[test]
fn postman_preserves_environment_variables_in_form_and_query_until_execution() {
    let imported = parse_import(
        &json!({"item": [{"request": {
            "method": "POST",
            "url": {"raw": "https://example.test", "protocol": "https", "host": "example.test", "query": [
                {"key": "search", "value": "{{term}}"},
                {"key": "literal", "value": "a%2Fb+c"}
            ]},
            "body": {"mode": "urlencoded", "urlencoded": [{"key": "token", "value": "{{token}}"}]}
        }}]})
        .to_string(),
    )
    .unwrap();
    let template = &imported[0].request;
    let resolved = request::resolve_variables(
        template,
        &std::collections::HashMap::from([
            ("term".into(), "one&two=three".into()),
            ("token".into(), "a+b".into()),
        ]),
    )
    .unwrap();
    let mut url = url::Url::parse(&resolved.path).unwrap();
    url.query_pairs_mut()
        .extend_pairs(resolved.query.as_ref().unwrap());

    assert_eq!(
        url.as_str(),
        "https://example.test/?search=one%26two%3Dthree&literal=a%2Fb+c"
    );
    assert_eq!(
        resolved.form,
        Some(FormBody::UrlEncoded(vec![("token".into(), "a+b".into())]))
    );
    assert_eq!(
        template.form,
        Some(FormBody::UrlEncoded(vec![(
            "token".into(),
            "{{token}}".into()
        )]))
    );
}

#[test]
fn postman_imports_multipart_file_arrays_and_text_fields() {
    let imported = parse_import(&json!({"item": [{"request": {
        "method": "POST", "url": "https://example.test/upload",
        "body": {"mode": "formdata", "formdata": [
            {"key": "description", "type": "text", "value": "{{description}}"},
            {"key": "files", "type": "file", "src": ["/nonexistent/a.bin", "/nonexistent/b.bin"]},
            {"key": "disabled", "type": "file", "src": null, "disabled": true}
        ]}
    }}]}).to_string()).unwrap();

    assert_eq!(
        imported[0].request.form,
        Some(FormBody::Multipart(vec![
            MultipartField::Text {
                name: "description".into(),
                value: "{{description}}".into()
            },
            MultipartField::File {
                name: "files".into(),
                path: "/nonexistent/a.bin".into()
            },
            MultipartField::File {
                name: "files".into(),
                path: "/nonexistent/b.bin".into()
            },
        ]))
    );
}

#[test]
fn postman_limits_total_expanded_data() {
    let collection = json!({
        "variable": [{"key": "large", "value": "x".repeat(1024 * 1024)}],
        "item": (0..17).map(|index| json!({"name": index.to_string(), "request": {
            "url": "https://example.test", "body": {"mode": "raw", "raw": "{{large}}"}
        }})).collect::<Vec<_>>()
    });

    assert!(
        parse_import(&collection.to_string())
            .unwrap_err()
            .contains("16 MiB")
    );
}

#[test]
fn postman_rejects_ambiguous_query_templates_and_unsupported_upload_names() {
    let query = json!({"item": [{"request": {"url": {
        "raw": "https://example.test", "protocol": "https", "host": "example.test", "query": [
            {"key": "term", "value": "{{term}}"},
            {"key": "literal", "value": "%7B%7Btoken%7D%7D"}
        ]
    }}}]});
    assert!(
        parse_import(&query.to_string())
            .unwrap_err()
            .contains("percent-encoded braces")
    );

    let upload = json!({"item": [{"request": {
        "url": "https://example.test", "body": {"mode": "formdata", "formdata": [
            {"key": "upload", "type": "file", "src": "/tmp/original.txt", "fileName": "custom.txt"}
        ]}
    }}]});
    assert!(
        parse_import(&upload.to_string())
            .unwrap_err()
            .contains("filenames")
    );
}

#[test]
fn postman_path_variables_preserve_extensions_and_parameter_names() {
    let imported = parse_import(&json!({"item": [{"request": {"url": {
        "protocol": "https", "host": ["example", "test"],
        "path": ["users", ":id.json", ":id-name.tar.gz", ":missing.json", ":empty.json"],
        "variable": [{"key": "id", "value": "42"}, {"key": "id-name", "value": "99"}, {"key": "empty", "value": ""}]
    }}}]}).to_string()).unwrap();

    assert_eq!(
        imported[0].request.path,
        "https://example.test/users/42.json/99.tar.gz/:missing.json/:empty.json"
    );
}

#[test]
fn postman_structured_string_paths_consume_only_the_leading_separator() {
    for (path, expected) in [
        (json!("/v1/users"), "/v1/users"),
        (json!("v1/users"), "/v1/users"),
        (json!("//v1/users"), "//v1/users"),
        (json!("/"), "/"),
        (json!(["", "v1", "users"]), "//v1/users"),
    ] {
        let imported = parse_import(
            &json!({"item": [{"request": {"url": {
                "protocol": "https", "host": ["example", "test"], "path": path
            }}}]})
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            imported[0].request.path,
            format!("https://example.test{expected}")
        );
    }
}

#[test]
fn curl_materializes_http_inference_without_using_the_editor_https_default() {
    for (source, expected) in [
        ("localhost:8080/path", "http://localhost:8080/path"),
        ("localhost:8080/{{id}}", "http://localhost:8080/{{id}}"),
        (
            "localhost:{{port}}/items",
            "http://localhost:{{port}}/items",
        ),
        ("api.{{domain}}/items", "http://api.{{domain}}/items"),
        (
            "example.test/?q={{query}}",
            "http://example.test/?q={{query}}",
        ),
        ("{{base_url}}/items", "{{base_url}}/items"),
        (
            "{{scheme}}://example.test/items",
            "{{scheme}}://example.test/items",
        ),
        ("example.test/path", "http://example.test/path"),
        ("[::1]:8080/path", "http://[::1]:8080/path"),
        (
            "ftp.user@example.test/path",
            "http://ftp.user@example.test/path",
        ),
        (
            "example.test/path?next=https://other.test",
            "http://example.test/path?next=https://other.test",
        ),
        (
            "HTTPS:/ftp.example.test/path",
            "https://ftp.example.test/path",
        ),
        ("http:///example.test/path", "http://example.test/path"),
    ] {
        let imported = parse_import(&format!("curl '{source}'")).unwrap();
        assert_eq!(imported[0].request.path, expected);
    }

    for source in [
        "ftp.example.test/path",
        "FTP.example.test/path",
        "%66tp.example.test/path",
        "user:pass@ftp.example.test/path",
        "dict.example.test",
        "ldap.example.test",
        "imap.example.test",
        "smtp.example.test",
        "pop3.example.test",
        "ftp://example.test",
        "//example.test/path",
        "::1/path",
    ] {
        assert!(
            parse_import(&format!("curl '{source}'")).is_err(),
            "Unexpectedly imported {source}"
        );
    }
}

#[test]
fn curl_rejects_url_globs_without_collapsing_multiple_requests() {
    for command in [
        "curl 'http://127.0.0.1:1/{a,b}'",
        "curl 'http://127.0.0.1:1/[1-2]'",
        "curl 'http://127.0.0.1:1/[a-z]'",
        "curl 'http://127.0.0.1:1/?tag[]=one'",
        "curl 'http://[::1]:1/{a,b}'",
        "curl '{{base_url}}/{a,b}'",
        "curl 'http://{{host}}/[1-2]'",
    ] {
        assert!(
            parse_import(command).unwrap_err().contains("globbing"),
            "Unexpected result for {command}"
        );
    }

    for command in [
        "curl --globoff 'http://127.0.0.1:1/{a,b}'",
        "curl 'http://127.0.0.1:1/{a,b}' --globoff",
        "curl -g 'http://127.0.0.1:1/[1-2]'",
        "curl 'http://127.0.0.1:1/[1-2]' -g",
        "curl 'http://[::1]:1/path'",
        "curl '[::1]:8080/path'",
        "curl 'http://[{{address}}]:8080/path'",
        "curl '{{base_url}}/items'",
        "curl 'http://{{host}}/items/{{id}}'",
    ] {
        assert!(
            parse_import(command).is_ok(),
            "Unexpectedly rejected {command}"
        );
    }
}

#[test]
fn curl_bearer_authentication_wins_over_user_in_both_orders() {
    for options in [
        "--oauth2-bearer token --user sam:pass",
        "--user sam:pass --oauth2-bearer token",
        "--oauth2-bearer previous --user sam:pass --oauth2-bearer token",
    ] {
        let imported = parse_import(&format!("curl http://localhost:8080 {options}")).unwrap();

        assert_eq!(
            imported[0].request.authentication,
            Authentication::Bearer {
                token: "token".into()
            }
        );
    }
}

#[test]
fn curl_combines_cookie_options_but_respects_explicit_cookie_headers() {
    for options in [
        "-b 'a=1' -H 'Cookie: b=2'",
        "-H 'cookie: b=2' -b 'a=1'",
        "-b 'a=1' -b 'c=3' -H 'Cookie: b=2'",
    ] {
        let imported = parse_import(&format!("curl http://localhost:8080 {options}")).unwrap();
        let headers = &imported[0].request.headers;

        assert_eq!(headers.len(), 1);
        assert!(headers[0].0.eq_ignore_ascii_case("cookie"));
        assert_eq!(headers[0].1, "b=2");
    }

    let imported = parse_import("curl http://localhost:8080 -b 'a=1' -b 'b=2'").unwrap();
    assert_eq!(
        imported[0].request.headers,
        [("Cookie".into(), "a=1;b=2".into())]
    );
}

#[test]
fn curl_multipart_rejects_content_type_overrides_that_form_encoding_cannot_preserve() {
    for content_type in [
        "application/vnd.example.form",
        "multipart/mixed",
        "multipart/form-data; boundary=custom",
        "multipart/form-data; charset=UTF-8",
        "{{content_type}}",
    ] {
        for options in [
            format!("-H 'Content-Type: {content_type}' -F 'message=hello'"),
            format!("--form-string 'message=hello' -H 'content-type: {content_type}'"),
        ] {
            let error = parse_import(&format!("curl https://example.test {options}")).unwrap_err();
            assert!(
                error.contains("custom Content-Type"),
                "Unexpected error for {options}: {error}"
            );
        }
    }

    for options in [
        "-F 'message=hello'",
        "-H 'Content-Type: multipart/form-data' -F 'message=hello'",
        "--form-string 'message=hello' -H 'content-type:  MuLtIpArT/FoRm-DaTa  '",
    ] {
        let imported = parse_import(&format!("curl https://example.test {options}")).unwrap();
        assert!(matches!(
            imported[0].request.form,
            Some(FormBody::Multipart(_))
        ));
    }
}

#[test]
fn import_rejects_bodies_that_the_selected_method_cannot_send() {
    for source in [
        "curl -X GET --data-raw 'payload' https://example.test/items",
        "curl -X HEAD -F 'name=value' https://example.test/items",
        r#"{"item":[{"name":"GET with body","request":{"method":"GET","url":"https://example.test/items","body":{"mode":"raw","raw":"payload"}}}]}"#,
    ] {
        assert!(parse_import(source).is_err(), "{source}");
    }
    assert!(parse_import("curl -G -d 'name=value' https://example.test/items").is_ok());
}
