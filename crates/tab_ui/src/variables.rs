use std::path::{Path, PathBuf};

use collection::{Collection, RequestScripts};
use environment::{Environment, EnvironmentLoadError, EnvironmentSession, VariableValues};
use gpui_kit::{App, Entity};

use crate::Environments;

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
    pub session: EnvironmentSession,
    pub environments: Option<Entity<Environments>>,
}

impl VariableScope {
    /// Reload file values so external edits appear on the next send or
    /// completion. The active global environment overrides the collection's.
    fn file_values(&self, cx: &App) -> Result<VariableValues, String> {
        let mut values = VariableValues::default();
        let active = self
            .environments
            .as_ref()
            .and_then(|environments| environments.read(cx).active_path());

        for path in self.path.iter().chain(active.iter()) {
            values.environment.extend(read_entries(path)?);
        }

        Ok(values)
    }

    pub fn values(&self, cx: &App) -> Result<VariableValues, String> {
        self.file_values(cx)
            .map(|values| self.session.values(values))
    }

    pub fn request_variables(&self, cx: &App) -> request::RequestVariables {
        let (values, error) = match self.file_values(cx) {
            Ok(values) => (values, None),
            Err(error) => (VariableValues::default(), Some(error)),
        };

        request::RequestVariables::with_environment_session(values, error, self.session.clone())
            .with_collection_scripts(self.collection_scripts())
    }

    /// Reload the collection's scripts so saved edits apply to the next send.
    fn collection_scripts(&self) -> Result<RequestScripts, String> {
        match self.path.as_deref().and_then(Path::parent) {
            Some(collection) => {
                Collection::load_scripts(collection).map_err(|error| error.to_string())
            }
            None => Ok(RequestScripts::default()),
        }
    }
}

fn read_entries(path: &Path) -> Result<std::collections::HashMap<String, String>, String> {
    match Environment::from_file(path) {
        Ok(environment) => Ok(environment.entries),
        Err(EnvironmentLoadError::Read { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(Default::default())
        }
        Err(error) => Err(error.to_string()),
    }
}
