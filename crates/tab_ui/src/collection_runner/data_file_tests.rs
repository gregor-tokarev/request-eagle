use std::path::Path;

use super::data_file::{DataRow, parse};

fn row(values: &[(&str, serde_json::Value)]) -> DataRow {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

#[test]
fn csv_rows_are_named_by_the_header() {
    let rows = parse(
        Path::new("users.csv"),
        "\u{feff}name, email\nAda,ada@example.com\n\"Lovelace, A\",\"say \"\"hi\"\"\"\nShort\n"
            .as_bytes(),
    )
    .unwrap();

    assert_eq!(
        rows,
        [
            row(&[("name", "Ada".into()), ("email", "ada@example.com".into())]),
            row(&[
                ("name", "Lovelace, A".into()),
                ("email", "say \"hi\"".into())
            ]),
            row(&[("name", "Short".into()), ("email", "".into())]),
        ]
    );
}

#[test]
fn json_values_keep_their_types() {
    let rows = parse(
        Path::new("data.txt"),
        br#" [{"id": 7, "active": false, "name": "Ada", "tags": ["a"], "none": null}]"#,
    )
    .unwrap();

    assert_eq!(
        rows,
        [row(&[
            ("id", 7.into()),
            ("active", false.into()),
            ("name", "Ada".into()),
            ("tags", serde_json::json!(["a"])),
            ("none", serde_json::Value::Null),
        ])]
    );
}

#[test]
fn files_without_rows_or_objects_are_refused() {
    assert_eq!(
        parse(Path::new("empty.csv"), b"name,email\n"),
        Err("The data file has no rows.".into())
    );
    assert_eq!(
        parse(Path::new("data.json"), br#"{"id": 1}"#),
        Err("A JSON data file must be an array of objects.".into())
    );
    assert_eq!(
        parse(Path::new("data.json"), br#"[{"id": 1}, 2]"#),
        Err("Item 2 of the JSON data file is not an object.".into())
    );
    assert!(parse(Path::new("data.json"), b"[").is_err());
    assert!(parse(Path::new("data.csv"), &[0xff, 0xfe]).is_err());
}
