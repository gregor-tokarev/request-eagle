use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};

/// The values of each scope scripts can change, or the changes to them.
/// `None` hides a name in its scope and in the scopes beneath it: the
/// environment covers the collection's variables, which cover the globals.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableScopes {
    pub globals: BTreeMap<String, Option<String>>,
    pub collection: BTreeMap<String, Option<String>>,
    pub environment: BTreeMap<String, Option<String>>,
}

impl VariableScopes {
    /// The values `{{name}}` resolves to.
    pub fn values(&self) -> HashMap<String, String> {
        let mut values = HashMap::new();

        for scope in [&self.globals, &self.collection, &self.environment] {
            for (name, value) in scope {
                match value {
                    Some(value) => values.insert(name.clone(), value.clone()),
                    None => values.remove(name),
                };
            }
        }

        values
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        for scope in [
            &mut self.globals,
            &mut self.collection,
            &mut self.environment,
        ] {
            scope.retain(|name, _| keep(name));
        }
    }
}

/// Changes to one scope, with a count of committed updates so tabs sharing
/// the session notice them.
#[derive(Debug, Default)]
struct Changes {
    values: Mutex<BTreeMap<String, Option<String>>>,
    revision: AtomicU64,
}

impl Changes {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Option<String>>> {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// File values with the session's changes over them.
    fn over(&self, base: HashMap<String, String>) -> BTreeMap<String, Option<String>> {
        let mut values: BTreeMap<_, _> = base
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect();
        values.extend(self.lock().clone());

        values
    }
}

/// Script changes that remain available for the workspace session without
/// changing the environment file. Global changes are shared by every
/// collection of the workspace; collection variable and environment changes
/// belong to one collection.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentSession {
    globals: Arc<Changes>,
    collection: Arc<Changes>,
    environment: Arc<Changes>,
}

impl EnvironmentSession {
    /// Changes whenever any scope of the session changes.
    pub fn revision(&self) -> u64 {
        [&self.globals, &self.collection, &self.environment]
            .iter()
            .map(|changes| changes.revision.load(Ordering::Acquire))
            .sum()
    }

    /// Each scope's file values with the session's changes over them.
    /// Globals have no file.
    pub fn scopes(
        &self,
        collection: HashMap<String, String>,
        environment: HashMap<String, String>,
    ) -> VariableScopes {
        VariableScopes {
            globals: self.globals.over(HashMap::new()),
            collection: self.collection.over(collection),
            environment: self.environment.over(environment),
        }
    }

    /// The values `{{name}}` resolves to.
    pub fn values(
        &self,
        collection: HashMap<String, String>,
        environment: HashMap<String, String>,
    ) -> HashMap<String, String> {
        self.scopes(collection, environment).values()
    }

    /// Commit only keys changed by a successful script phase. Independent
    /// requests can update different keys without replacing each other's work.
    /// Reject the entire update when a scope's names and values exceed 1 MiB
    /// or it has more than 4096 changed names, including deletions.
    pub fn apply(&self, changes: &VariableScopes) -> Result<(), &'static str> {
        // Every session locks the shared globals first, so sessions of
        // different collections cannot wait on each other.
        let mut globals = self.globals.lock();
        let mut collection = self.collection.lock();
        let mut environment = self.environment.lock();

        if !within_limits(&globals, &changes.globals)
            || !within_limits(&collection, &changes.collection)
            || !within_limits(&environment, &changes.environment)
        {
            return Err(
                "Session variables exceed the limit of 4096 changed values or 1 MiB in a scope",
            );
        }

        for (name, value) in &changes.globals {
            // Nothing lies beneath the globals for a deletion to hide.
            match value {
                Some(value) => globals.insert(name.clone(), Some(value.clone())),
                None => globals.remove(name),
            };
        }
        collection.extend(changes.collection.clone());
        environment.extend(changes.environment.clone());
        drop((globals, collection, environment));

        for (scope, changes) in [
            (&self.globals, &changes.globals),
            (&self.collection, &changes.collection),
            (&self.environment, &changes.environment),
        ] {
            if !changes.is_empty() {
                scope.revision.fetch_add(1, Ordering::AcqRel);
            }
        }

        Ok(())
    }
}

fn within_limits(
    current: &BTreeMap<String, Option<String>>,
    changes: &BTreeMap<String, Option<String>>,
) -> bool {
    let mut entries = current.len();
    let mut bytes = current
        .iter()
        .map(|(name, value)| name.len() + value.as_ref().map_or(0, String::len))
        .sum::<usize>();

    for (name, value) in changes {
        if let Some(previous) = current.get(name) {
            bytes = bytes.saturating_sub(previous.as_ref().map_or(0, String::len));
        } else {
            entries += 1;
            bytes = bytes.saturating_add(name.len());
        }

        bytes = bytes.saturating_add(value.as_ref().map_or(0, String::len));
    }

    entries <= 4096 && bytes <= 1024 * 1024
}

/// A workspace owns this registry; all of its tabs share the session for their
/// collection environment path. Unattached tabs share a separate session.
/// Every session of the registry shares its globals.
#[derive(Clone, Default)]
pub struct EnvironmentSessions {
    scopes: Arc<Mutex<HashMap<Option<PathBuf>, EnvironmentSession>>>,
    globals: Arc<Changes>,
}

impl EnvironmentSessions {
    pub fn for_path(&self, path: Option<&Path>) -> EnvironmentSession {
        self.scopes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(path.map(Path::to_path_buf))
            .or_insert_with(|| EnvironmentSession {
                globals: self.globals.clone(),
                ..Default::default()
            })
            .clone()
    }
}
