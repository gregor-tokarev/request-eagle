use std::path::PathBuf;

use environment::{Environment, EnvironmentLoadError, VariableValues};

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
}

impl VariableScope {
    /// Read the collection environment without retaining values between sends.
    /// Unattached requests and collections without an environment use only generated variables.
    pub fn values(&self) -> Result<VariableValues, String> {
        let Some(path) = &self.path else {
            return Ok(VariableValues::default());
        };

        match Environment::from_file(path) {
            Ok(environment) => Ok(VariableValues {
                environment: environment.entries,
            }),
            Err(EnvironmentLoadError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(VariableValues::default())
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
