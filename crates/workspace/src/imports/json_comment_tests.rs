use serde_json::{Value, json};

use super::parse_import;

fn imported_body(raw: &str, language: Option<&str>, content_type: Option<&str>) -> String {
    let mut request = json!({
        "method": "POST", "url": "https://example.test", "body": {"mode": "raw", "raw": raw}
    });

    if let Some(language) = language {
        request["body"]["options"] = json!({"raw": {"language": language}});
    }

    if let Some(content_type) = content_type {
        request["header"] = json!([{"key": "Content-Type", "value": content_type}]);
    }

    let imported = parse_import(&json!({"item": [{"request": request}]}).to_string()).unwrap();
    String::from_utf8(imported[0].request.body.clone().unwrap()).unwrap()
}

#[test]
fn postman_json_comments_follow_language_and_content_type_activation() {
    let raw = "{ /* comment */ \"ok\": true }";
    let stripped = "{  \"ok\": true }";

    for (language, content_type, expected) in [
        (Some("json"), None, stripped),
        (Some("json"), Some("text/plain"), stripped),
        (None, Some("application/json"), stripped),
        (
            Some(""),
            Some("application/vnd.api+json; charset=utf-8"),
            stripped,
        ),
        (None, Some("application/jsonish"), stripped),
        (None, None, raw),
        (None, Some("text/plain"), raw),
        (Some("text"), Some("application/json"), raw),
        (Some("JSON"), Some("application/json"), raw),
        (None, Some("Application/json"), raw),
        (None, Some("application/JSON"), raw),
        (None, Some(" application/json"), raw),
    ] {
        assert_eq!(
            imported_body(raw, language, content_type),
            expected,
            "language={language:?} content_type={content_type:?}"
        );
    }
}

#[test]
fn postman_json_comments_preserve_strings_escapes_unicode_and_templates() {
    let raw = r#"{
  "url": "https://example.test/a//b", // trailing comment
  "marker": "/* literal */",
  "quote": "say \"//literal\"",
  "slashes": "\\", /* actual comment */
  "template": "{{name}}",
  "unicode": "Привет 🌍"
}"#;
    let expected = concat!(
        r#"{
  "url": "https://example.test/a//b", "#,
        "\n",
        r#"  "marker": "/* literal */",
  "quote": "say \"//literal\"",
  "slashes": "\\", "#,
        "\n",
        r#"  "template": "{{name}}",
  "unicode": "Привет 🌍"
}"#
    );

    assert_eq!(imported_body(raw, Some("json"), None), expected);
    assert!(serde_json::from_str::<Value>(expected).is_ok());
}

#[test]
fn postman_json_comments_match_whitespace_and_unterminated_comment_behavior() {
    for (raw, expected) in [
        ("A//comment\nB", "A\nB"),
        ("A//comment\r\nB", "A\nB"),
        ("A//comment\rB", "A"),
        ("A//comment\u{2028}B", "A"),
        ("A/*comment\nline*/B", "AB"),
        ("A/*comment\r\nline*/B", "AB"),
        ("A /*comment*/ B", "A  B"),
        ("A/*unterminated\nB", "A"),
        ("A/* nested /* stops here */B", "AB"),
        ("A\r\nB/*comment*/\r\nC", "A\r\nB\r\nC"),
    ] {
        assert_eq!(imported_body(raw, Some("json"), None), expected);
    }
}

#[test]
fn postman_non_json_and_form_bodies_keep_literal_comment_text() {
    let raw = "// text\r\n/* content */ {{value}}";

    assert_eq!(
        imported_body(raw, Some("text"), Some("application/json")),
        raw
    );

    let imported = parse_import(
        &json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": "Content-Type", "value": "application/json"}],
            "body": {"mode": "urlencoded", "urlencoded": [{"key": "value", "value": raw}]}
        }}]})
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        imported[0].request.form,
        Some(request::FormBody::UrlEncoded(vec![(
            "value".into(),
            raw.into()
        )]))
    );
}

#[test]
fn postman_comment_preparation_retains_disabled_header_activation_without_sending_them() {
    let raw = "{ /* comment */ \"ok\": true }";
    let stripped = "{  \"ok\": true }";

    for (headers, expected, outgoing_count) in [
        (
            json!([{"key": "Content-Type", "value": "application/json", "disabled": true}]),
            stripped,
            1,
        ),
        (
            json!([{"key": "Content-Type", "value": "text/plain", "disabled": true}]),
            raw,
            1,
        ),
        (
            json!([
                {"key": "Content-Type", "value": "text/plain", "disabled": true},
                {"key": "Content-Type", "value": "text/plain", "disabled": true}
            ]),
            stripped,
            1,
        ),
        (
            json!([
                {"key": "Content-Type", "value": "text/plain", "disabled": true},
                {"key": "Content-Type", "value": "application/json"}
            ]),
            stripped,
            1,
        ),
        (
            json!([
                {"key": "Content-Type", "value": "text/plain"},
                {"key": "Content-Type", "value": "application/json"}
            ]),
            raw,
            2,
        ),
    ] {
        let imported = parse_import(
            &json!({"item": [{"request": {
                "method": "POST", "url": "https://example.test", "header": headers,
                "body": {"mode": "raw", "raw": raw}
            }}]})
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(expected.as_bytes())
        );
        assert_eq!(imported[0].request.headers.len(), outgoing_count);
    }
}

#[test]
fn postman_rejects_actual_comments_when_content_type_processing_remains_dynamic() {
    for raw in ["{\n //comment\n \"a\":1\n}", "{ /*comment*/ \"a\":1 }"] {
        for headers in [
            json!([{"key": "Content-Type", "value": "{{ct}}"}]),
            json!([{"key": "Content-Type", "value": "application/{{subtype}}"}]),
            json!([{"key": "Content-Type", "value": "{{ct}}", "disabled": true}]),
            json!("Content-Type: {{ct}}"),
            json!([{"key": "{{header_name}}", "value": "application/json"}]),
            json!([{"key": "{{header_name}}", "value": "application/json", "disabled": true}]),
        ] {
            for language in [None, Some("")] {
                let mut body = json!({"mode": "raw", "raw": raw});

                if let Some(language) = language {
                    body["options"] = json!({"raw": {"language": language}});
                }

                let source = json!({"item": [{"request": {
                    "method": "POST", "url": "https://example.test", "header": headers, "body": body
                }}]});
                let error = parse_import(&source.to_string()).unwrap_err();
                assert!(
                    error.contains("dynamic Content-Type")
                        || error.contains("Postman header names must be resolved before importing"),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn postman_dynamic_content_type_keeps_comment_like_json_strings_and_explicit_languages() {
    let raw =
        r#"{"url":"https://example.test/a//b","marker":"/*literal*/","quote":"say \"//literal\""}"#;
    assert_eq!(imported_body(raw, None, Some("{{ct}}")), raw);

    let commented = "{ /*comment*/ \"a\":1 }";
    for (language, expected) in [
        ("json", "{  \"a\":1 }"),
        ("text", commented),
        ("javascript", commented),
    ] {
        assert_eq!(
            imported_body(commented, Some(language), Some("{{ct}}")),
            expected
        );

        let source = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": "Content-Type", "value": "text/plain"},
                {"key": "{{header_name}}", "value": "{{ct}}", "disabled": true}],
            "body": {"mode": "raw", "raw": commented, "options": {"raw": {"language": language}}}
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(expected.as_bytes())
        );
    }
}

#[test]
fn postman_comment_processing_keeps_static_selection_and_resolved_collection_defaults() {
    let raw = "{ /*comment*/ \"a\":1 }";
    for (content_type, expected) in [("application/json", "{  \"a\":1 }"), ("text/plain", raw)] {
        let source = json!({"item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": "Content-Type", "value": content_type},
                {"key": "Content-Type", "value": "{{irrelevant}}"}],
            "body": {"mode": "raw", "raw": raw}
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(expected.as_bytes())
        );

        let source = json!({"variable": [{"key": "ct", "value": content_type}], "item": [{"request": {
            "method": "POST", "url": "https://example.test",
            "header": [{"key": "Content-Type", "value": "{{ct}}"}],
            "body": {"mode": "raw", "raw": raw}
        }}]});
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(expected.as_bytes())
        );
    }
}
