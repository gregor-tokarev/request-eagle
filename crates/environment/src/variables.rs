use std::collections::HashMap;

use rand::{
    Rng,
    distr::{Alphanumeric, SampleString},
};
use thiserror::Error;

pub const GENERATED_VARIABLES: &[(&str, &str)] = &[
    ("$guid", "A new UUID v4"),
    ("$isoTimestamp", "Current UTC time in ISO 8601 format"),
    ("$timestamp", "Current Unix time in seconds"),
    ("$randomInt", "An integer from 0 to 1000"),
    ("$randomBoolean", "true or false"),
    ("$randomAlphaNumeric", "A random letter or digit"),
    ("$randomEmail", "A random address at example.com"),
];

/// Values stay separate from completion metadata and are never written into drafts.
#[derive(Clone, Default)]
pub struct VariableValues {
    pub environment: HashMap<String, String>,
}

pub fn valid_variable_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VariableError {
    #[error("Unknown variable {{{{{0}}}}}. Check the name and collection environment.")]
    Unknown(String),
    #[error("Unclosed variable. Complete the reference with }}}} before sending.")]
    Unclosed,
    #[error("Expanded request exceeds the output limit")]
    TooLarge,
}

/// One resolver per send keeps repeated generated references consistent in that request.
pub struct VariableResolver<'a> {
    values: &'a VariableValues,
    generated: HashMap<String, String>,
    remaining: usize,
}

impl<'a> VariableResolver<'a> {
    pub fn new(values: &'a VariableValues) -> Self {
        Self {
            values,
            generated: HashMap::new(),
            remaining: usize::MAX,
        }
    }

    pub fn limit_output(&mut self, bytes: usize) {
        self.remaining = bytes;
    }

    fn append(&mut self, result: &mut String, text: &str) -> Result<(), VariableError> {
        self.remaining = self
            .remaining
            .checked_sub(text.len())
            .ok_or(VariableError::TooLarge)?;
        result.push_str(text);
        Ok(())
    }

    pub fn resolve(&mut self, text: &str) -> Result<String, VariableError> {
        let mut result = String::new();
        let mut remaining = text;

        while let Some(start) = remaining.find("{{") {
            self.append(&mut result, &remaining[..start])?;
            remaining = &remaining[start + 2..];
            let end = remaining.find("}}").ok_or(VariableError::Unclosed)?;
            if let Some(literal) = remaining[..end].strip_prefix('!') {
                self.append(&mut result, "{{")?;
                self.append(&mut result, literal)?;
                self.append(&mut result, "}}")?;
            } else {
                let name = remaining[..end].trim();
                let value = self.value(name)?;
                self.append(&mut result, &value)?;
            }
            remaining = &remaining[end + 2..];
        }

        self.append(&mut result, remaining)?;
        Ok(result)
    }

    fn value(&mut self, name: &str) -> Result<String, VariableError> {
        if name.starts_with('$') {
            if let Some(value) = self.generated.get(name) {
                return Ok(value.clone());
            }

            let value = match name {
                "$guid" | "$randomUUID" => uuid::Uuid::new_v4().to_string(),
                "$isoTimestamp" => {
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                }
                "$timestamp" => chrono::Utc::now().timestamp().to_string(),
                "$randomInt" => rand::rng().random_range(0..=1000).to_string(),
                "$randomBoolean" => rand::rng().random::<bool>().to_string(),
                "$randomAlphaNumeric" => Alphanumeric.sample_string(&mut rand::rng(), 1),
                "$randomEmail" => format!(
                    "{}@example.com",
                    Alphanumeric.sample_string(&mut rand::rng(), 12)
                ),
                _ => return Err(VariableError::Unknown(name.into())),
            };
            self.generated.insert(name.into(), value.clone());
            return Ok(value);
        }

        self.values
            .environment
            .get(name)
            .cloned()
            .ok_or_else(|| VariableError::Unknown(name.into()))
    }
}
