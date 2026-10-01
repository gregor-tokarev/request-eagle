use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use collection::Collection;

use environment::{Environment, EnvironmentSession};
use gpui_kit::{App, Context, Entity};
use request::RequestScripts;

use crate::Environments;

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
    pub session: EnvironmentSession,
    pub environments: Option<Entity<Environments>>,
    /// Names that resolve and the session revision they were read at,
    /// shared by every field of the request.
    pub names: Option<(u64, Rc<HashSet<String>>)>,
}

impl VariableScope {
    /// The files, session or active environment may have changed.
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.names = None;
        cx.notify();
    }

    /// Environment names a reference resolves to. Other tabs of the collection
    /// can change the shared session, so its revision is checked every time.
    pub fn names(&mut self, cx: &App) -> Rc<HashSet<String>> {
        let revision = self.session.revision();
        if let Some((read_at, names)) = &self.names
            && *read_at == revision
        {
            return names.clone();
        }

        // Sending still resolves session values when a file can't be read.
        let values = self
            .values(cx)
            .unwrap_or_else(|_| self.session.values(HashMap::new()));
        let names = Rc::new(values.into_keys().collect::<HashSet<_>>());
        self.names = Some((revision, names.clone()));

        names
    }

    /// The collection's environment file and the active global one, which
    /// overrides it.
    fn files(&self, cx: &App) -> Vec<PathBuf> {
        let active = self
            .environments
            .as_ref()
            .and_then(|environments| environments.read(cx).active_path());

        self.path.iter().cloned().chain(active).collect()
    }

    /// When each environment file was last changed. A change made outside the
    /// app changes these without notifying the scope.
    pub fn file_versions(&self, cx: &App) -> Vec<Option<SystemTime>> {
        self.files(cx)
            .iter()
            .map(|path| {
                std::fs::metadata(path)
                    .and_then(|file| file.modified())
                    .ok()
            })
            .collect()
    }

    /// Reload file values so external edits appear on the next send or
    /// completion. The active global environment overrides the collection's.
    fn file_values(&self, cx: &App) -> Result<HashMap<String, String>, String> {
        let mut values = HashMap::new();

        for path in self.files(cx) {
            values.extend(read_entries(&path)?);
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
