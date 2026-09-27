use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::VariableValues;

/// Script changes that remain available for the workspace session without
/// changing the environment file. Deleted values mask their file-backed value.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentSession {
    changes: Arc<Mutex<BTreeMap<String, Option<String>>>>,
}

impl EnvironmentSession {
    pub fn values(&self, mut base: VariableValues) -> VariableValues {
        let changes = self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        for (name, value) in changes.iter() {
            if let Some(value) = value {
                base.environment.insert(name.clone(), value.clone());
            } else {
                base.environment.remove(name);
            }
        }

        base
    }

    /// Commit only keys changed by a successful script phase. Independent
    /// requests can update different keys without replacing each other's work.
    /// Reject the entire update when names and values exceed 1 MiB or there are
    /// more than 4096 changed names, including deletions.
    pub fn apply(&self, changes: &BTreeMap<String, Option<String>>) -> Result<(), &'static str> {
        let mut current = self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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

        if entries > 4096 || bytes > 1024 * 1024 {
            return Err("Session environment exceeds its limit of 4096 changed values or 1 MiB");
        }

        current.extend(
            changes
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );

        Ok(())
    }
}

/// A workspace owns this registry; all of its tabs share the session for their
/// collection environment path. Unattached tabs share a separate session.
#[derive(Clone, Default)]
pub struct EnvironmentSessions {
    scopes: Arc<Mutex<HashMap<Option<PathBuf>, EnvironmentSession>>>,
}

impl EnvironmentSessions {
    pub fn for_path(&self, path: Option<&Path>) -> EnvironmentSession {
        self.scopes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(path.map(Path::to_path_buf))
            .or_default()
            .clone()
    }
}
