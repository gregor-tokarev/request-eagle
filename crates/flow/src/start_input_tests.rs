use serde_json::{Value, json};

use crate::{JsonError, StartInput, parse_start_input};

fn invalid(text: &str) -> JsonError {
    match StartInput::check(text) {
        StartInput::InvalidJson(error) => error,
        other => panic!("{text:?} parsed as {other:?}"),
    }
}

#[test]
fn empty_and_whitespace_inputs_send_null_without_a_notice() {
    for text in ["", "   ", "\n\t  \r\n"] {
        let (value, kind) = parse_start_input(text);

        assert_eq!(value, Value::Null, "{text:?}");
        assert_eq!(kind, StartInput::Empty, "{text:?}");
        assert_eq!(StartInput::check(text), StartInput::Empty, "{text:?}");
        assert_eq!(kind.run_notice(), None, "{text:?}");
    }
}

#[test]
fn valid_json_values_have_no_notice() {
    let cases = [
        (r#"{"name":"Gregor"}"#, json!({"name": "Gregor"})),
        ("{\n  \"name\": \"Gregor\"\n}", json!({"name": "Gregor"})),
        ("[1, 2, 3]", json!([1, 2, 3])),
        ("\"hello\"", json!("hello")),
        ("\"\"", json!("")),
        ("0", json!(0)),
        ("42", json!(42)),
        ("-3.25", json!(-3.25)),
        ("1e5", json!(1e5)),
        ("true", json!(true)),
        ("false", json!(false)),
        ("null", Value::Null),
        ("  {\"ok\": true}  \n", json!({"ok": true})),
    ];

    for (text, expected) in cases {
        let (value, kind) = parse_start_input(text);

        assert_eq!(value, expected, "{text:?}");
        assert_eq!(kind, StartInput::Json, "{text:?}");
        assert_eq!(StartInput::check(text), StartInput::Json, "{text:?}");
        assert_eq!(kind.run_notice(), None, "{text:?}");
    }
}

#[test]
fn malformed_json_reports_its_position_and_sends_raw_text() {
    let text = r#"{"name":"Gregor",}"#;
    let (value, kind) = parse_start_input(text);

    assert_eq!(value, Value::String(text.to_owned()));
    assert_eq!(
        kind,
        StartInput::InvalidJson(JsonError {
            message: "trailing comma".to_owned(),
            line: 1,
            column: 18,
        })
    );
    assert_eq!(
        kind.run_notice().as_deref(),
        Some("Invalid JSON, sent as text (line 1, column 18: trailing comma)")
    );

    let array = invalid("[1, 2,]");
    assert_eq!(
        array.to_string(),
        "Invalid JSON at line 1, column 7: trailing comma"
    );

    let unquoted_key = invalid(r#"{name: "Gregor"}"#);
    assert_eq!((unquoted_key.line, unquoted_key.column), (1, 2));

    let trailing = invalid(r#"{"a": 1} extra"#);
    assert_eq!((trailing.line, trailing.column), (1, 10));
    assert_eq!(trailing.message, "trailing characters");

    let unclosed_string = invalid("\"hello");
    assert_eq!((unclosed_string.line, unclosed_string.column), (1, 6));
    assert!(unclosed_string.message.starts_with("EOF while parsing"));
}

#[test]
fn multiline_and_unicode_errors_keep_accurate_line_and_column() {
    let next_line = invalid("{\n  \"имя\": \"Грегор\",\n}");
    let same_line = invalid(r#"{"имя": "Грегор" "город": 1}"#);
    let eof_after_newlines = invalid("{\n  \"name\": \"Gregor\"\n\n  \n");
    let eof_after_crlf = invalid("{\r\n  \"name\": \"Gregor\"\r\n\r\n");
    let newline_in_string = invalid("{\"s\": \"a\nb\",\n\"x\": 0}");
    let crlf_in_string = invalid("{\"s\": \"a\r\nb\",\r\n\"x\": 0}");
    let empty_next_line = invalid("\"a\n");
    let leading_blank_line = invalid("\n  {\"name\":\"Gregor\",}");

    assert_eq!((next_line.line, next_line.column), (3, 1));
    assert_eq!((same_line.line, same_line.column), (1, 18));
    assert_eq!(
        (eof_after_newlines.line, eof_after_newlines.column),
        (2, 18)
    );
    assert_eq!((eof_after_crlf.line, eof_after_crlf.column), (2, 18));
    assert_eq!((newline_in_string.line, newline_in_string.column), (1, 8));
    assert_eq!((crlf_in_string.line, crlf_in_string.column), (1, 8));
    assert_eq!((empty_next_line.line, empty_next_line.column), (1, 2));
    assert_eq!(
        (leading_blank_line.line, leading_blank_line.column),
        (2, 20)
    );
}

#[test]
fn plain_text_sends_verbatim_with_a_text_notice() {
    for text in [
        "hello",
        "  hello world  ",
        "https://example.com",
        "truee",
        "01",
        "1, 2",
        "1e9999",
        "name: \"Gregor\"",
    ] {
        let (value, kind) = parse_start_input(text);

        assert_eq!(value, Value::String(text.to_owned()), "{text:?}");
        assert_eq!(kind, StartInput::Text, "{text:?}");
        assert_eq!(StartInput::check(text), StartInput::Text, "{text:?}");
        assert_eq!(kind.run_notice().as_deref(), Some("Not JSON, sent as text"));
    }
}
