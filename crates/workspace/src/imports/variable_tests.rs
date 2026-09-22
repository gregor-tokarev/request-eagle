use serde_json::{Value, json};

use super::parse_import;

fn collection(variables: Value, body: &str) -> Value {
    json!({"variable": variables, "item": [{"request": {
        "method": "POST", "url": "https://example.test",
        "body": {"mode": "raw", "raw": body}
    }}]})
}

// Postman Runtime 7.56.1 substitutes null as "null", missing as empty, and
// ordinary defaults through JavaScript string conversion. Typed variables can
// coerce credentials or JSON values, so only matching declared types are allowed.
#[test]
fn collection_defaults_preserve_supported_values_and_unresolved_names() {
    for (variable, expected) in [
        (json!({"key": "value", "value": null}), "null"),
        (json!({"key": "value"}), ""),
        (json!({"key": "value", "value": "01"}), "01"),
        (json!({"key": "value", "value": true}), "true"),
        (json!({"key": "value", "value": 3}), "3"),
        (
            json!({"key": "value", "type": "any", "value": null}),
            "null",
        ),
        (json!({"key": "value", "type": null, "value": null}), "null"),
        (
            json!({"key": "value", "type": "string", "value": "01"}),
            "01",
        ),
        (json!({"key": "value", "type": "NUMBER", "value": 1}), "1"),
        (
            json!({"key": "value", "type": "Boolean", "value": false}),
            "false",
        ),
    ] {
        let source = collection(
            json!([variable]),
            "{\"value\":{{value}},\"other\":\"{{unknown}}\"}",
        );
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(format!("{{\"value\":{expected},\"other\":\"{{{{unknown}}}}\"}}").as_bytes()),
            "{source}"
        );
    }
}

#[test]
fn collection_defaults_reject_type_coercion_instead_of_changing_raw_bodies() {
    for variable in [
        json!({"key": "value", "type": "number", "value": "01"}),
        json!({"key": "value", "type": "boolean", "value": "false"}),
        json!({"key": "value", "type": "string", "value": 123}),
        json!({"key": "value", "type": "number", "value": true}),
        json!({"key": "value", "type": "number", "value": null}),
        json!({"key": "value", "type": "boolean", "value": null}),
        json!({"key": "value", "type": "string", "value": null}),
        json!({"key": "value", "type": "number"}),
        json!({"key": "value", "type": "boolean"}),
        json!({"key": "value", "type": "string"}),
        json!({"key": "value", "type": " number", "value": 1}),
        json!({"key": "value", "type": "object", "value": {}}),
        json!({"key": "value", "type": 1, "value": "value"}),
    ] {
        let source = collection(json!([variable]), "{\"value\":{{value}}}");
        let error = parse_import(&source.to_string()).unwrap_err();
        assert!(
            error.contains("variable") && error.contains("type"),
            "{error}"
        );
    }
}

#[test]
fn collection_numeric_defaults_reject_floats_and_integers_outside_javascript_safe_range() {
    for value in [
        json!(1.0),
        json!(0.000001),
        json!(-0.000001),
        json!(9_007_199_254_740_992_i64),
        json!(-9_007_199_254_740_992_i64),
        json!(u64::MAX),
    ] {
        for declared_type in [None, Some("any"), Some("number")] {
            let mut variable = json!({"key": "value", "value": value});

            if let Some(declared_type) = declared_type {
                variable["type"] = json!(declared_type);
            }

            let source = collection(json!([variable]), "{{value}}");
            let error = parse_import(&source.to_string()).unwrap_err();
            assert!(error.contains("unsupported numeric default"), "{error}");
        }
    }
}

#[test]
fn collection_numeric_defaults_preserve_javascript_safe_integer_boundaries() {
    for value in [-9_007_199_254_740_991_i64, -1, 0, 1, 9_007_199_254_740_991] {
        for declared_type in [None, Some("any"), Some("number")] {
            let mut variable = json!({"key": "value", "value": value});

            if let Some(declared_type) = declared_type {
                variable["type"] = json!(declared_type);
            }

            let source = collection(json!([variable]), "{{value}}");
            let imported = parse_import(&source.to_string()).unwrap();
            assert_eq!(
                imported[0].request.body.as_deref(),
                Some(value.to_string().as_bytes()),
                "{source}"
            );
        }
    }
}

#[test]
fn collection_defaults_preserve_numeric_text_in_strings() {
    for value in ["1.0", "0.000001", "1e-6", "9007199254740992"] {
        for declared_type in [None, Some("any"), Some("string")] {
            let mut variable = json!({"key": "value", "value": value});

            if let Some(declared_type) = declared_type {
                variable["type"] = json!(declared_type);
            }

            let source = collection(json!([variable]), "{{value}}");
            let imported = parse_import(&source.to_string()).unwrap();
            assert_eq!(
                imported[0].request.body.as_deref(),
                Some(value.as_bytes()),
                "{source}"
            );
        }
    }
}

#[test]
fn collection_defaults_use_the_last_enabled_definition_without_merging_types() {
    for (last, expected) in [
        (json!({"key": "value", "value": "01"}), "01"),
        (json!({"key": "value", "value": null}), "null"),
        (json!({"key": "value"}), ""),
        (
            json!({"key": "value", "value": "unused", "disabled": true}),
            "1",
        ),
    ] {
        let source = collection(
            json!([
                {"key": "value", "type": "number", "value": 1}, last
            ]),
            "value={{value}}",
        );
        let imported = parse_import(&source.to_string()).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(format!("value={expected}").as_bytes())
        );
    }

    let source = collection(
        json!([
            {"key": "value", "type": "number", "value": "01"},
            {"key": "value", "type": "number", "value": 1.0},
            {"key": "value", "value": "literal"}
        ]),
        "{{value}}",
    );
    let imported = parse_import(&source.to_string()).unwrap();
    assert_eq!(
        imported[0].request.body.as_deref(),
        Some(b"literal".as_slice())
    );
}
