use serde_json::json;

use crate::template::render;

#[test]
fn fills_variables_and_dotted_names_without_escaping() {
    let data = json!({"name": "<Eagle>", "user": {"id": 7, "tags": ["a", "b"]}, "missing": null});

    assert_eq!(
        render(
            "Hi {{ name }} #{{user.id}} {{user.tags.1}}{{missing}}{{nothing}}",
            &data
        )
        .unwrap(),
        "Hi <Eagle> #7 b"
    );
    assert_eq!(
        render("{{{name}}} {{& name}}", &data).unwrap(),
        "<Eagle> <Eagle>"
    );
}

#[test]
fn renders_objects_and_lists_as_json() {
    let data = json!({"user": {"id": 7}, "list": [1, "two"]});

    assert_eq!(
        render("{{user}} {{list}}", &data).unwrap(),
        "{\"id\":7} [1,\"two\"]"
    );
}

#[test]
fn repeats_sections_for_lists_and_skips_falsy_ones() {
    let data = json!({
        "items": [{"id": 1}, {"id": 2}],
        "empty": [],
        "flag": true,
        "off": false,
        "user": {"name": "Ada"},
        "words": ["x", "y"]
    });

    assert_eq!(
        render(
            "{{#items}}<{{id}}>{{/items}}{{#empty}}never{{/empty}}",
            &data
        )
        .unwrap(),
        "<1><2>"
    );
    assert_eq!(
        render(
            "{{#flag}}on{{/flag}}{{#off}}off{{/off}}{{^off}}not off{{/off}}",
            &data
        )
        .unwrap(),
        "onnot off"
    );
    assert_eq!(render("{{#user}}{{name}}{{/user}}", &data).unwrap(), "Ada");
    assert_eq!(render("{{#words}}{{.}},{{/words}}", &data).unwrap(), "x,y,");
    // Outer names stay visible inside a section.
    assert_eq!(
        render("{{#items}}{{user.name}}{{id}} {{/items}}", &data).unwrap(),
        "Ada1 Ada2 "
    );
    assert_eq!(render("a{{! a comment }}b", &data).unwrap(), "ab");
}

#[test]
fn reports_unbalanced_tags() {
    let data = json!({});

    assert!(render("{{#a}}", &data).unwrap_err().contains("not closed"));
    assert!(
        render("{{/a}}", &data)
            .unwrap_err()
            .contains("closes no section")
    );
    assert!(
        render("{{#a}}{{/b}}", &data)
            .unwrap_err()
            .contains("closed by")
    );
    assert!(render("{{name", &data).unwrap_err().contains("not closed"));
}
