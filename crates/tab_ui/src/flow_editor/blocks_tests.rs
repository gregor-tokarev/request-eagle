use flow::StartInput;

use super::blocks::{schema_summary, start_input_caption};

#[test]
fn summarizes_what_a_schema_checks() {
    assert_eq!(
        schema_summary(r#"{"type": "object", "required": ["body", "id"]}"#),
        Ok("object · requires body, id".to_owned())
    );
    assert_eq!(
        schema_summary(r#"{"type": ["string", "null"]}"#),
        Ok("string or null".to_owned())
    );
    assert_eq!(
        schema_summary(r#"{"properties": {"a": {}, "b": {}}}"#),
        Ok("2 properties".to_owned())
    );
    assert_eq!(schema_summary("{}"), Ok("Checks a JSON Schema".to_owned()));
    assert_eq!(schema_summary("true"), Ok("Accepts anything".to_owned()));
    assert!(schema_summary("{\"type\": ").is_err());
    assert!(schema_summary("[1]").is_err());
}

#[test]
fn start_caption_explains_non_json_inputs() {
    assert_eq!(start_input_caption(&StartInput::check("")), None);
    assert_eq!(
        start_input_caption(&StartInput::check(r#"{"name":"Gregor"}"#)),
        None
    );
    assert_eq!(start_input_caption(&StartInput::check("42")), None);
    assert_eq!(
        start_input_caption(&StartInput::check(r#"{"name":"Gregor",}"#)),
        Some("Invalid JSON · sends as text")
    );
    assert_eq!(
        start_input_caption(&StartInput::check("hello")),
        Some("Not JSON · sends as text")
    );
}
