use std::path::PathBuf;

use environment::{Environment, EnvironmentLoadError, EnvironmentSession, VariableValues};

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
    pub session: EnvironmentSession,
}

impl VariableScope {
    /// Reload file values so external edits appear on the next send or completion.
    fn file_values(&self) -> Result<VariableValues, String> {
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

    pub fn values(&self) -> Result<VariableValues, String> {
        self.file_values().map(|values| self.session.values(values))
    }

    pub fn request_variables(&self) -> request::RequestVariables {
        let (values, error) = match self.file_values() {
            Ok(values) => (values, None),
            Err(error) => (VariableValues::default(), Some(error)),
        };

        request::RequestVariables::with_environment_session(values, error, self.session.clone())
    }
}
