use rquickjs::{Context, Runtime};
use serde_json::{Value, json};

fn evaluate(source: &str) -> Result<String, String> {
    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();

    context.with(|cx| {
        cx.globals()
            .set("utilities", super::utilities::bindings(cx.clone()).unwrap())
            .unwrap();

        cx.eval::<String, _>(source)
            .map_err(|error| rquickjs::CaughtError::from_error(&cx, error).to_string())
    })
}

fn validate(data: Value, schema: Value) -> Result<Value, String> {
    let source = format!(
        "utilities.validateSchema({}, {})",
        json!(data.to_string()),
        json!(schema.to_string()),
    );

    evaluate(&source).map(|result| serde_json::from_str(&result).unwrap())
}

#[test]
fn hashing_and_hmac_match_known_vectors() {
    assert_eq!(
        evaluate("utilities.sha256('abc')").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        evaluate("utilities.sha256('')").unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        evaluate("utilities.hmacSha256('Jefe', 'what do ya want for nothing?')").unwrap(),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn encoding_round_trips_utf8_and_base64url_omits_padding() {
    assert_eq!(
        evaluate("utilities.base64Encode('Hello 🌍')").unwrap(),
        "SGVsbG8g8J+MjQ=="
    );
    assert_eq!(
        evaluate("utilities.base64Decode('SGVsbG8g8J+MjQ==')").unwrap(),
        "Hello 🌍"
    );
    assert_eq!(
        evaluate("utilities.base64UrlEncode('Hello 🌍')").unwrap(),
        "SGVsbG8g8J-MjQ"
    );

    for encoded in ["SGVsbG8g8J-MjQ", "SGVsbG8g8J-MjQ=="] {
        assert_eq!(
            evaluate(&format!("utilities.base64UrlDecode('{}')", encoded)).unwrap(),
            "Hello 🌍"
        );
    }

    assert_eq!(evaluate("utilities.base64Encode('')").unwrap(), "");
    assert!(evaluate("utilities.base64Decode('%%%')").is_err());
    assert!(
        evaluate("utilities.base64Decode('/w==')")
            .unwrap_err()
            .contains("UTF-8")
    );
    assert_eq!(
        evaluate(
            "utilities.base64Decode(utilities.base64Encode('x'.repeat(1048576))).length.toString()"
        )
        .unwrap(),
        "1048576"
    );
}

#[test]
fn crypto_rejects_invalid_types_and_bounds_native_allocations() {
    for source in [
        "utilities.sha256(123)",
        "utilities.hmacSha256(null, 'hello')",
        "utilities.base64Encode({})",
        "utilities.sha256('x'.repeat(1048577))",
        "utilities.base64Decode('x'.repeat(1048577))",
        "utilities.randomBytes(-1)",
        "utilities.randomBytes(0.5)",
        "utilities.randomBytes(NaN)",
        "utilities.randomBytes(Infinity)",
        "utilities.randomBytes(65537)",
        "utilities.randomBytes('32')",
    ] {
        assert!(evaluate(source).is_err(), "{source} should fail");
    }

    assert_eq!(evaluate("utilities.randomBytes(0)").unwrap(), "");

    let bytes = evaluate("utilities.randomBytes(32)").unwrap();
    assert_eq!(bytes.len(), 64);
    assert!(bytes.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(bytes, evaluate("utilities.randomBytes(32)").unwrap());
}

#[test]
fn schema_validates_reusable_response_shapes_and_formats() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["users"],
        "properties": {
            "users": {"type": "array", "items": {"$ref": "#/$defs/user"}},
        },
        "$defs": {
            "user": {
                "type": "object",
                "required": ["id", "email"],
                "properties": {
                    "id": {"type": "integer", "minimum": 1},
                    "email": {"type": "string", "format": "email"},
                },
                "additionalProperties": false,
            }
        },
    });

    let result = validate(
        json!({"users": [{"id": 1, "email": "eagle@example.com"}]}),
        schema.clone(),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"valid": true, "errors": [], "truncated": false})
    );

    let result = validate(
        json!({"users": [{"id": 0, "email": "invalid", "extra": true}]}),
        schema,
    )
    .unwrap();
    assert_eq!(result["valid"], false);

    let errors = result["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 3);
    assert!(
        errors
            .iter()
            .any(|error| error["instancePath"] == "/users/0/id")
    );
    assert!(
        errors
            .iter()
            .any(|error| error["instancePath"] == "/users/0/email")
    );
    assert!(errors.iter().all(|error| {
        error["schemaPath"].as_str().unwrap().starts_with('/')
            && !error["message"].as_str().unwrap().is_empty()
    }));
}

#[test]
fn schema_supports_boolean_schemas_draft7_and_literal_reference_properties() {
    assert_eq!(validate(json!(null), json!(true)).unwrap()["valid"], true);
    assert_eq!(validate(json!(null), json!(false)).unwrap()["valid"], false);

    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "array",
        "items": [{"const": "hello"}, {"type": "integer"}],
        "additionalItems": false,
        "examples": [{"$ref": "https://example.com/literal"}],
    });
    assert_eq!(
        validate(json!(["hello", 42]), schema.clone()).unwrap()["valid"],
        true
    );
    assert_eq!(
        validate(json!(["hello", 42, true]), schema).unwrap()["valid"],
        false
    );
}

#[test]
fn schema_failures_are_capped_and_invalid_schema_throws() {
    let result = validate(
        json!(vec![false; 100]),
        json!({"items": {"type": "string"}}),
    )
    .unwrap();
    assert_eq!(result["valid"], false);
    assert_eq!(result["truncated"], true);
    assert_eq!(result["errors"].as_array().unwrap().len(), 20);

    assert!(validate(json!(null), json!({"type": "invalid"})).is_err());
    assert!(validate(json!(null), json!({"pattern": "(?=lookahead)"})).is_err());
}

#[test]
fn schema_cannot_retrieve_external_files_or_urls_or_follow_cyclic_references() {
    for schema in [
        json!({"$ref": "https://127.0.0.1:1/schema"}),
        json!({"$ref": "file:///etc/passwd"}),
        json!({"$ref": "other.json"}),
        json!({"$ref": "#"}),
        json!({"$ref": "#/$defs/self", "$defs": {"self": {"$ref": "#/$defs/self"}}}),
        json!({"$dynamicRef": "#root"}),
        json!({"$recursiveRef": "#"}),
        json!({"$defs": {"other": {"$id": "nested"}}}),
    ] {
        assert!(
            validate(json!(null), schema.clone()).is_err(),
            "{schema} should fail"
        );
    }
}

#[test]
fn schema_reference_graphs_and_json_sizes_are_bounded() {
    let mut definitions = serde_json::Map::new();
    definitions.insert("end".into(), json!({"type": "string"}));
    let mut last = "end".to_owned();

    for index in 0..14 {
        let next = index.to_string();
        definitions.insert(
            next.clone(),
            json!({"allOf": [{"$ref": format!("#/$defs/{last}")}, {"$ref": format!("#/$defs/{last}")}]}),
        );
        last = next;
    }

    assert!(validate(json!("hello"), json!({"$defs": definitions})).is_err());
    assert!(validate(json!("x".repeat(1024 * 1024)), json!(true)).is_err());
    assert!(validate(json!(null), json!({"description": "x".repeat(64 * 1024)})).is_err());
    assert!(validate(json!(vec![true; 20_001]), json!(true)).is_err());

    let mut data = json!(null);

    for _ in 0..65 {
        data = json!([data]);
    }

    assert!(validate(data, json!(true)).is_err());
}
