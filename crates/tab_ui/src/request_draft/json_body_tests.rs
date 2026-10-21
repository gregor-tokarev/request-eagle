use super::json_body::{JsonBody, JsonError};

fn error(text: &str) -> JsonError {
    match JsonBody::check(text) {
        JsonBody::Invalid(error) => error,
        other => panic!("{text:?} is {other:?}"),
    }
}

#[test]
fn a_trailing_comma_is_explained_where_it_is() {
    assert_eq!(
        error(r#"{"name":"Gregor",}"#).to_string(),
        "Invalid JSON at line 1, column 18: trailing comma"
    );
}

#[test]
fn columns_count_characters_on_the_error_line() {
    let next_line = error("{\n  \"имя\": \"Грегор\",\n}");
    let same_line = error(r#"{"имя": "Грегор" "город": 1}"#);

    assert_eq!((next_line.line, next_line.column), (3, 1));
    assert_eq!((same_line.line, same_line.column), (1, 18));
}

#[test]
fn an_unfinished_body_points_at_its_last_character() {
    let error = error("{\n  \"name\": \"Gregor\"\n\n  \n");

    assert_eq!((error.line, error.column), (2, 18));
    assert!(error.message.starts_with("EOF while parsing"));
}

#[test]
fn empty_and_valid_bodies_have_no_error() {
    assert_eq!(JsonBody::check(""), JsonBody::Empty);
    assert_eq!(JsonBody::check(" \n "), JsonBody::Empty);
    assert_eq!(JsonBody::check(r#"{"name": "{{name}}"}"#), JsonBody::Valid);
}

#[test]
fn variables_in_place_of_values_are_not_errors() {
    for text in [
        r#"{"id": {{id}}, "tags": [{{first}}, {{ second }}]}"#,
        r#"{"user": {{user}}, "total": -{{amount}}.5}"#,
        "{{body}}",
        r#"{"id": {{$randomInt}}{{suffix}}}"#,
        r#"{"amount": 1e{{power}}}"#,
    ] {
        assert_eq!(JsonBody::check(text), JsonBody::Templated, "{text}");
    }
}

#[test]
fn errors_beside_variables_keep_their_position() {
    let error = error(r#"{"id": {{id}}, "name": "{{name}}",}"#);

    assert_eq!(error.message, "trailing comma");
    assert_eq!((error.line, error.column), (1, 35));
}

#[test]
fn literal_and_unclosed_references_are_checked_as_written() {
    assert!(matches!(
        JsonBody::check(r#"{"id": {{!id}}}"#),
        JsonBody::Invalid(_)
    ));
    assert!(matches!(
        JsonBody::check("{\"id\": {{id\n}}"),
        JsonBody::Invalid(_)
    ));
}
