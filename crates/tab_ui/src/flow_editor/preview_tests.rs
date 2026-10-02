use flow::DisplayFormat;
use serde_json::json;

use super::preview::{Display, compact, pretty};

#[test]
fn compact_values_fit_on_one_line_within_their_limit() {
    assert_eq!(compact(&json!({"a": [1, 2]}), 100), "{\"a\":[1,2]}");
    assert_eq!(compact(&json!("two\nlines"), 100), "two lines");
    assert_eq!(compact(&json!("ééééé"), 3), "é…");
}

#[test]
fn pretty_values_are_cut_to_a_screenful() {
    let long: Vec<u32> = (0..200).collect();
    let text = pretty(&json!(long));

    assert!(text.lines().count() <= 61);
    assert!(text.ends_with('…'));
    assert_eq!(pretty(&json!("plain")), "plain");
}

#[test]
fn display_blocks_show_tables_for_lists_of_objects() {
    let rows = json!([{"id": 1, "name": "Ada"}, {"id": 2, "role": "admin"}]);

    let Display::Table {
        columns,
        rows,
        more,
    } = Display::new(&rows, DisplayFormat::Auto)
    else {
        panic!("expected a table");
    };
    assert_eq!(columns, ["id", "name", "role"]);
    assert_eq!(rows[0], ["1", "Ada", ""]);
    assert_eq!(more, 0);

    let Display::Table { columns, rows, .. } = Display::new(&json!({"a": 1}), DisplayFormat::Table)
    else {
        panic!("expected a table");
    };
    assert_eq!(columns, ["key", "value"]);
    assert_eq!(rows, [["a", "1"]]);

    assert_eq!(
        Display::new(&json!("text"), DisplayFormat::Json),
        Display::Text("\"text\"".into())
    );
    assert_eq!(
        Display::new(&json!("text"), DisplayFormat::Auto),
        Display::Text("text".into())
    );
    assert_eq!(
        Display::new(&json!(5), DisplayFormat::Table),
        Display::Text("5".into())
    );
}
