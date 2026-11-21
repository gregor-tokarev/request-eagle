use std::fmt;

use serde_json::Value;

/// How a Start block's inline input is read when a run has no input of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartInput {
    /// Blank or whitespace only: sends `null`.
    Empty,
    /// Valid JSON: sends the parsed value.
    Json,
    /// Looks like JSON with a syntax error: sends the raw text as a string.
    InvalidJson(JsonError),
    /// Plain text rather than JSON: sends the raw text as a string.
    Text,
}

impl StartInput {
    /// How a Start block's inline input will be read when the flow runs.
    pub fn check(text: &str) -> Self {
        parse_start_input(text).1
    }

    /// Why a Start block's inline input was sent as text rather than JSON, if
    /// it was. Empty and valid JSON inputs have no notice.
    pub fn run_notice(&self) -> Option<String> {
        match self {
            Self::Empty | Self::Json => None,
            Self::InvalidJson(error) => Some(format!(
                "Invalid JSON, sent as text (line {}, column {}: {})",
                error.line, error.column, error.message
            )),
            Self::Text => Some("Not JSON, sent as text".to_owned()),
        }
    }
}

/// Parse a Start block's inline input into the value it sends and how it was
/// read.
pub fn parse_start_input(text: &str) -> (Value, StartInput) {
    if text.trim().is_empty() {
        return (Value::Null, StartInput::Empty);
    }

    match serde_json::from_str::<Value>(text) {
        Ok(value) => (value, StartInput::Json),
        Err(error) if looks_like_json(text) => (
            Value::String(text.to_owned()),
            StartInput::InvalidJson(JsonError::new(text, &error)),
        ),
        Err(_) => (Value::String(text.to_owned()), StartInput::Text),
    }
}

/// Whether non-JSON text starts like a JSON object, array or string, where a
/// syntax error points at a real mistake rather than plain text.
fn looks_like_json(text: &str) -> bool {
    matches!(
        text.trim_start().as_bytes().first(),
        Some(b'{' | b'[' | b'"')
    )
}

/// A JSON syntax error at a 1-based line and column. Columns count characters,
/// as the editor does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl JsonError {
    fn new(text: &str, error: &serde_json::Error) -> Self {
        let message = error.to_string();
        let position = format!(" at line {} column {}", error.line(), error.column());
        let message = message
            .strip_suffix(&position)
            .unwrap_or(&message)
            .to_owned();

        // At the end of input that ends in line breaks, serde_json points at
        // the start of the line after them, where the editor has nothing to
        // underline. Point at the last non-whitespace character instead.
        if error.is_eof() && error.column() == 0 {
            let content = text.trim_end();
            let last_line = content.rsplit('\n').next().unwrap_or("");

            return Self {
                message,
                line: content.matches('\n').count() + 1,
                column: last_line.chars().count().max(1),
            };
        }

        // A literal line break inside a string advances serde_json to column 0
        // of the next line. Point at the end of the line that broke, where the
        // editor has a visible character to underline (matching CRLF errors).
        if error.column() == 0 && error.line() > 1 {
            let previous_line = error.line() - 1;
            let line = text.lines().nth(previous_line - 1).unwrap_or("");

            return Self {
                message,
                line: previous_line,
                column: line.chars().count().max(1),
            };
        }

        // serde_json counts the column in bytes.
        let line = text
            .lines()
            .nth(error.line().saturating_sub(1))
            .unwrap_or("");
        let column = line
            .char_indices()
            .take_while(|(index, _)| *index < error.column())
            .count();

        Self {
            message,
            line: error.line().max(1),
            column: column.max(1),
        }
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid JSON at line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}
