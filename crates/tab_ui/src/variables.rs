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
            .unwrap_or_else(|_| self.session.values(HashMap::new(), HashMap::new()));
        let names = Rc::new(values.into_keys().collect::<HashSet<_>>());
        self.names = Some((revision, names.clone()));

        names
    }

    /// Reload the collection's variables, so external edits appear on the
    /// next send or completion.
    fn collection_values(&self) -> Result<HashMap<String, String>, String> {
        self.path
            .as_deref()
            .map_or_else(|| Ok(HashMap::new()), read_entries)
    }

    /// Reload the active global environment the same way.
    fn environment_values(&self, cx: &App) -> Result<HashMap<String, String>, String> {
        let active = self
            .environments
            .as_ref()
            .and_then(|environments| environments.read(cx).active_path());

        active
            .as_deref()
            .map_or_else(|| Ok(HashMap::new()), read_entries)
    }

    /// The values `{{name}}` resolves to. The active global environment
    /// overrides the collection's variables.
    pub fn values(&self, cx: &App) -> Result<HashMap<String, String>, String> {
        Ok(self
            .session
            .values(self.collection_values()?, self.environment_values(cx)?))
    }

    pub fn request_variables(&self, cx: &App) -> request::RequestVariables {
        let files = self
            .collection_values()
            .and_then(|collection| Ok((collection, self.environment_values(cx)?)));
        let ((collection, environment), error) = match files {
            Ok(files) => (files, None),
            Err(error) => (Default::default(), Some(error)),
        };

        request::RequestVariables::with_environment_session(
            collection,
            environment,
            error,
            self.session.clone(),
        )
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
