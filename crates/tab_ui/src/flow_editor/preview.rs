//! Short text for values that runs produce, prepared once when they arrive
//! rather than every time the canvas draws.

use std::io;

use flow::DisplayFormat;
use gpui_kit::SharedString;
use serde_json::Value;

const TEXT_LIMIT: usize = 4096;
const LINE_LIMIT: usize = 60;
const ROW_LIMIT: usize = 50;
const COLUMN_LIMIT: usize = 8;
const CELL_LIMIT: usize = 80;

/// JSON on one line, cut at about `limit` bytes.
pub(super) fn compact(value: &Value, limit: usize) -> SharedString {
    let text = match value {
        Value::String(text) => truncate(text, limit).replace('\n', " "),
        value => json(value, false, limit),
    };

    text.into()
}

/// Indented JSON, cut to a screenful.
pub(super) fn pretty(value: &Value) -> String {
    let text = match value {
        Value::String(text) => truncate(text, TEXT_LIMIT),
        value => json(value, true, TEXT_LIMIT),
    };

    let mut lines = text.lines();
    let mut kept: String = lines
        .by_ref()
        .take(LINE_LIMIT)
        .collect::<Vec<_>>()
        .join("\n");
    if lines.next().is_some() {
        kept.push_str("\n…");
    }

    truncate(&kept, TEXT_LIMIT)
}

/// JSON of a value, cut at about `limit` bytes. Writing stops at the limit,
/// so a large response costs no more than a small one.
pub(super) fn json(value: &Value, indent: bool, limit: usize) -> String {
    let mut writer = Limited {
        bytes: Vec::new(),
        limit: limit + 1,
    };
    let _ = if indent {
        serde_json::to_writer_pretty(&mut writer, value)
    } else {
        serde_json::to_writer(&mut writer, value)
    };

    truncate(&String::from_utf8_lossy(&writer.bytes), limit)
}

/// Keeps the first `limit` bytes written to it, then refuses the rest.
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}

impl io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let room = self.limit - self.bytes.len();
        if room == 0 {
            return Err(io::Error::other("The preview is full"));
        }

        let count = bytes.len().min(room);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The start of `text`, at most about `limit` bytes, marked when cut.
fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }

    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// What a Display block shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Display {
    Text(SharedString),
    Table {
        columns: Vec<SharedString>,
        rows: Vec<Vec<SharedString>>,
        /// Rows left out to keep the table short.
        more: usize,
    },
}

impl Display {
    pub fn new(value: &Value, format: DisplayFormat) -> Self {
        match format {
            DisplayFormat::Json => Self::Text(pretty_json(value).into()),
            DisplayFormat::Text => Self::Text(match value {
                Value::String(text) => truncate(text, TEXT_LIMIT).into(),
                value => compact(value, TEXT_LIMIT),
            }),
            DisplayFormat::Table => {
                table(value).unwrap_or_else(|| Self::Text(pretty(value).into()))
            }
            DisplayFormat::Auto => match value {
                Value::Array(items) if items.iter().all(Value::is_object) && !items.is_empty() => {
                    table(value).unwrap_or_else(|| Self::Text(pretty(value).into()))
                }
                value => Self::Text(pretty(value).into()),
            },
        }
    }
}

/// JSON, also for a string, which shows its quotes.
fn pretty_json(value: &Value) -> String {
    match value {
        Value::String(_) => json(value, false, TEXT_LIMIT),
        value => pretty(value),
    }
}

fn table(value: &Value) -> Option<Display> {
    let cell = |value: &Value| -> SharedString { compact(value, CELL_LIMIT) };

    match value {
        Value::Array(items) if items.iter().all(Value::is_object) && !items.is_empty() => {
            let mut columns: Vec<String> = Vec::new();
            for item in items.iter().filter_map(Value::as_object) {
                for key in item.keys() {
                    if columns.len() < COLUMN_LIMIT && !columns.contains(key) {
                        columns.push(key.clone());
                    }
                }
            }

            let rows = items
                .iter()
                .take(ROW_LIMIT)
                .map(|item| {
                    columns
                        .iter()
                        .map(|column| item.get(column).map(cell).unwrap_or_default())
                        .collect()
                })
                .collect();

            Some(Display::Table {
                columns: columns.into_iter().map(SharedString::from).collect(),
                rows,
                more: items.len().saturating_sub(ROW_LIMIT),
            })
        }
        Value::Array(items) => Some(Display::Table {
            columns: vec!["value".into()],
            rows: items
                .iter()
                .take(ROW_LIMIT)
                .map(|item| vec![cell(item)])
                .collect(),
            more: items.len().saturating_sub(ROW_LIMIT),
        }),
        Value::Object(object) => Some(Display::Table {
            columns: vec!["key".into(), "value".into()],
            rows: object
                .iter()
                .take(ROW_LIMIT)
                .map(|(key, value)| vec![key.clone().into(), cell(value)])
                .collect(),
            more: object.len().saturating_sub(ROW_LIMIT),
        }),
        _ => None,
    }
}
