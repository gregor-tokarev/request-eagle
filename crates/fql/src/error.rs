use std::fmt;

/// A parse or evaluation error with JSONata's code, such as `S0201` for a
/// syntax error or `T2001` for arithmetic on a string, and where in the
/// expression it happened, counted in characters.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    pub position: Option<usize>,
}

impl Error {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            position: None,
        }
    }

    pub(crate) fn at(code: &'static str, position: usize, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            position: Some(position),
        }
    }

    /// Keep the innermost position an error has.
    pub(crate) fn or_at(mut self, position: usize) -> Self {
        self.position.get_or_insert(position);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.position {
            Some(position) => write!(
                formatter,
                "{}: {} (at character {})",
                self.code,
                self.message,
                position + 1
            ),
            None => write!(formatter, "{}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;
