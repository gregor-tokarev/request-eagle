use std::fmt;

use crate::variable_input::variable_references;

/// How a JSON body reads, which decides whether it can be formatted and
/// what the editor points out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum JsonBody {
    /// Nothing entered.
    Empty,
    /// JSON as written.
    Valid,
    /// JSON once its `{{variables}}` are filled in. Formatting needs JSON as
    /// written, so it cannot keep variables that stand in for values.
    Templated,
    /// Not JSON, even with its variables filled in. It can still be sent.
    Invalid(JsonError),
}

impl JsonBody {
    pub(super) fn check(text: &str) -> Self {
        if text.trim().is_empty() {
            return Self::Empty;
        }
        if serde_json::from_str::<serde_json::Value>(text).is_ok() {
            return Self::Valid;
        }

        // A number as long as the reference stands in for any value and fits
        // inside a string, so the error keeps its position in the text.
        let mut filled = text.to_owned();
        for range in variable_references(text) {
            filled.replace_range(range.clone(), &"1".repeat(range.len()));
        }

        match serde_json::from_str::<serde_json::Value>(&filled) {
            Ok(_) => Self::Templated,
            Err(error) => Self::Invalid(JsonError::new(text, &error)),
        }
    }
}

/// A syntax error at a 1-based line and column. Columns count characters,
/// as the editor does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JsonError {
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
            line: error.line(),
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
