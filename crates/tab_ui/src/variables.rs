use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use collection::Collection;

use environment::{Environment, EnvironmentSession};
use gpui_kit::{App, Context, Entity};
use request::RequestScripts;

use crate::Environments;

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
    pub session: EnvironmentSession,
    pub environments: Option<Entity<Environments>>,
    /// Names that resolve, read once after each change for every field.
    pub names: Option<Rc<HashSet<String>>>,
}

impl VariableScope {
    /// The files, session or active environment may have changed.
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.names = None;
        cx.notify();
    }

    /// Environment names a reference resolves to. A file that can't be read
    /// resolves nothing, as sending would fail.
    pub fn names(&mut self, cx: &App) -> Rc<HashSet<String>> {
        if self.names.is_none() {
            let names = self.values(cx).unwrap_or_default().into_keys().collect();
            self.names = Some(Rc::new(names));
        }

        self.names.clone().unwrap_or_default()
    }

    /// Reload file values so external edits appear on the next send or
    /// completion. The active global environment overrides the collection's.
    fn file_values(&self, cx: &App) -> Result<HashMap<String, String>, String> {
        let mut values = HashMap::new();
        let active = self
            .environments
            .as_ref()
            .and_then(|environments| environments.read(cx).active_path());

        for path in self.path.iter().chain(active.iter()) {
            values.extend(read_entries(path)?);
        }

        Ok(values)
    }

    pub fn values(&self, cx: &App) -> Result<HashMap<String, String>, String> {
        self.file_values(cx)
            .map(|values| self.session.values(values))
    }

    pub fn request_variables(&self, cx: &App) -> request::RequestVariables {
        let (values, error) = match self.file_values(cx) {
            Ok(values) => (values, None),
            Err(error) => (HashMap::new(), Some(error)),
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
    Environment::from_file(path)
        .map(|environment| environment.entries)
        .map_err(|error| error.to_string())
}
