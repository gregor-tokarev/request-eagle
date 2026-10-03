use super::blocks::schema_summary;

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
