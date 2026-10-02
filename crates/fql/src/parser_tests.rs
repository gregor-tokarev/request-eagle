use serde_json::json;

use crate::{Bindings, Expression, evaluate};

fn parse_error(source: &str) -> crate::Error {
    Expression::parse(source).expect_err(source)
}

#[test]
fn reads_literals_names_and_comments() {
    let check = |source: &str, expected| {
        assert_eq!(
            evaluate(
                source,
                Some(&json!({"a b": 1, "and": 2})),
                &Bindings::default()
            )
            .unwrap(),
            Some(expected),
            "{source}"
        );
    };

    check("'single' & \"double\"", json!("singledouble"));
    check("\"\\u00e9\\n\\\"\\ud83d\\ude00\"", json!("é\n\"😀"));
    check("1.5e3", json!(1500));
    check("`a b`", json!(1));
    check("and", json!(2));
    check("/* a comment */ 1 /* another */ + 1", json!(2));
    check("true and not", json!(false));
    check("null", json!(null));
}

#[test]
fn tells_division_from_regular_expressions() {
    let input = json!({"a": 10, "b": 2, "text": "a/b"});

    assert_eq!(
        evaluate("a / b", Some(&input), &Bindings::default()).unwrap(),
        Some(json!(5))
    );
    assert_eq!(
        evaluate("a/b", Some(&input), &Bindings::default()).unwrap(),
        Some(json!(5))
    );
    assert_eq!(
        evaluate("$split(text, /\\//)", Some(&input), &Bindings::default()).unwrap(),
        Some(json!(["a", "b"]))
    );
    assert_eq!(
        evaluate("$contains(text, /[/]/)", Some(&input), &Bindings::default()).unwrap(),
        Some(json!(true))
    );
}

#[test]
fn reports_syntax_errors_with_their_position() {
    let error = parse_error("1 +");
    assert_eq!(error.code, "S0203");

    let error = parse_error("a.(b");
    assert_eq!(error.code, "S0203");

    let error = parse_error("[1, 2");
    assert_eq!(error.code, "S0203");

    let error = parse_error("a b");
    assert_eq!(error.code, "S0201");
    assert_eq!(error.position, Some(2));

    assert_eq!(parse_error("'unterminated").code, "S0101");
    assert_eq!(parse_error("'bad \\q'").code, "S0103");
    assert_eq!(parse_error("`name").code, "S0105");
    assert_eq!(parse_error("/* open").code, "S0106");
    assert_eq!(parse_error("1 ! 2").code, "S0204");
    assert_eq!(parse_error("a.5").code, "S0213");
    assert_eq!(parse_error("function(x) { x }").code, "S0208");
    assert_eq!(parse_error("5 := 1").code, "S0212");
    assert_eq!(parse_error("a#b").code, "S0214");
    assert_eq!(parse_error("%.a").code, "S0217");
    assert_eq!(parse_error("$match('a', //)").code, "S0301");
    assert_eq!(parse_error("'x' ~> /a/q").code, "S0302");

    let message = parse_error("a b").to_string();
    assert!(message.starts_with("S0201: Syntax error"), "{message}");
    assert!(message.ends_with("(at character 3)"), "{message}");
}

#[test]
fn reads_function_signatures_and_partial_application() {
    let result = evaluate(
        "($f := function($a, $b)<nn:n> { $a - $b }; $g := $f(10, ?); $g(3))",
        None,
        &Bindings::default(),
    );
    assert_eq!(result.unwrap(), Some(json!(7)));
}
