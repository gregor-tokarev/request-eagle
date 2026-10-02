use std::{collections::BTreeMap, path::Path};

/// One iteration's values from a data file, by column or key name. CSV
/// values are text; JSON values keep their types for scripts.
pub(crate) type DataRow = BTreeMap<String, serde_json::Value>;

/// The rows of a Collection Runner data file: a JSON array of objects, or
/// CSV whose first row names the columns. A `.json` file is JSON; other
/// files are JSON when they start with `[`.
pub(crate) fn parse(path: &Path, bytes: &[u8]) -> Result<Vec<DataRow>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "The data file is not UTF-8 text.".to_owned())?
        .trim_start_matches('\u{feff}');
    let json = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        || text.trim_start().starts_with('[');

    let rows = if json {
        json_rows(text)?
    } else {
        csv_rows(text)?
    };

    if rows.is_empty() {
        return Err("The data file has no rows.".into());
    }

    Ok(rows)
}

fn json_rows(text: &str) -> Result<Vec<DataRow>, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("The data file is not valid JSON: {error}"))?;
    let serde_json::Value::Array(items) = value else {
        return Err("A JSON data file must be an array of objects.".into());
    };

    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            serde_json::Value::Object(fields) => Ok(fields.into_iter().collect()),
            _ => Err(format!(
                "Item {} of the JSON data file is not an object.",
                index + 1
            )),
        })
        .collect()
}

fn csv_rows(text: &str) -> Result<Vec<DataRow>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let names = reader
        .headers()
        .map_err(|error| format!("The CSV data file cannot be read: {error}"))?
        .iter()
        .map(|name| name.trim().to_owned())
        .collect::<Vec<_>>();

    reader
        .records()
        .map(|record| {
            let record =
                record.map_err(|error| format!("The CSV data file cannot be read: {error}"))?;

            // A short row leaves its last columns empty.
            Ok(names
                .iter()
                .enumerate()
                .filter(|(_, name)| !name.is_empty())
                .map(|(column, name)| (name.clone(), record.get(column).unwrap_or_default().into()))
                .collect())
        })
        .collect()
}
