use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use request::HttpRequest;
use serde::{Deserialize, Serialize};

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HistoryEntry {
    pub name: String,
    pub request: HttpRequest,
    pub environment_path: Option<PathBuf>,
    pub sent_at: u64,
}

impl HistoryEntry {
    pub fn new(name: String, request: HttpRequest, environment_path: Option<PathBuf>) -> Self {
        Self {
            name,
            request,
            environment_path,
            sent_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

#[derive(Default)]
pub(crate) struct History {
    pub entries: Vec<HistoryEntry>,
    path: Option<PathBuf>,
}

impl History {
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let entries = match fs::read(&path) {
            Ok(data) => {
                serde_json::from_slice::<Vec<HistoryEntry>>(&data).map_err(io::Error::other)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };

        Ok(Self {
            entries: entries.into_iter().take(HISTORY_LIMIT).collect(),
            path: Some(path),
        })
    }

    pub fn push(&mut self, entry: HistoryEntry) -> io::Result<()> {
        self.entries.insert(0, entry);
        self.entries.truncate(HISTORY_LIMIT);
        self.save()
    }

    pub fn relocate_environment(
        &mut self,
        previous_collection: &Path,
        environment: &Path,
    ) -> io::Result<()> {
        let mut changed = false;
        for entry in &mut self.entries {
            let Some(previous_environment) = &entry.environment_path else {
                continue;
            };
            let Some(collection) = previous_environment.parent() else {
                continue;
            };

            // The collection relocation event gives the original root. Do not
            // infer relocation from filesystem existence: case-only renames
            // leave the old spelling readable on case-insensitive filesystems.
            if collection == previous_collection {
                entry.environment_path = Some(environment.to_path_buf());
                changed = true;
            }
        }

        if changed {
            self.save()?;
        }
        Ok(())
    }

    pub fn clear(&mut self) -> io::Result<()> {
        // Keep the visible entries if deletion could not be persisted.
        if let Some(path) = &self.path {
            crate::session::write_private_json(path, &Vec::<HistoryEntry>::new())?;
        }

        self.entries.clear();
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        if let Some(path) = &self.path {
            crate::session::write_private_json(path, &self.entries)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_latest_snapshots_and_can_be_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut history = History::load(path.clone()).unwrap();

        for index in 0..105 {
            history
                .push(HistoryEntry::new(
                    index.to_string(),
                    HttpRequest {
                        path: format!("https://example.com/{index}"),
                        ..Default::default()
                    },
                    None,
                ))
                .unwrap();
        }

        let mut reloaded = History::load(path.clone()).unwrap();
        assert_eq!(reloaded.entries.len(), 100);
        assert_eq!(reloaded.entries[0].request.path, "https://example.com/104");
        assert_eq!(reloaded.entries[99].name, "5");
        reloaded.clear().unwrap();
        assert!(History::load(path).unwrap().entries.is_empty());
    }

    #[test]
    fn corrupt_history_is_not_silently_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        fs::write(&path, b"not json").unwrap();
        assert!(History::load(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"not json");
    }
}

#[cfg(test)]
mod relocation_tests {
    use super::{History, HistoryEntry};

    #[test]
    fn collection_relocation_updates_history_even_when_old_spelling_is_readable() {
        let directory = tempfile::tempdir().unwrap();
        let before = directory.path().join("API");
        let after = directory.path().join("api");
        std::fs::create_dir(&before).unwrap();
        let state = directory.path().join("history.json");
        let mut history = History::load(state.clone()).unwrap();
        history
            .push(HistoryEntry::new(
                "Item".into(),
                request::HttpRequest::default(),
                Some(before.join("environment.toml")),
            ))
            .unwrap();

        // Emulate the case-insensitive alias on every test platform.
        assert!(before.is_dir());
        history
            .relocate_environment(&before, &after.join("environment.toml"))
            .unwrap();
        assert_eq!(
            History::load(state).unwrap().entries[0].environment_path,
            Some(after.join("environment.toml")),
        );
    }

    #[test]
    fn collection_rename_updates_persisted_history_but_request_move_keeps_original_environment() {
        let directory = tempfile::tempdir().unwrap();
        let before = directory.path().join("Old API");
        let after = directory.path().join("Renamed API");
        let other = directory.path().join("Other API");
        std::fs::create_dir_all(&before).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let state = directory.path().join("history.json");
        let mut history = History::load(state.clone()).unwrap();
        history
            .push(HistoryEntry::new(
                "Item".into(),
                request::HttpRequest::default(),
                Some(before.join("environment.toml")),
            ))
            .unwrap();

        history
            .relocate_environment(&before.join("item.toml"), &other.join("environment.toml"))
            .unwrap();
        assert_eq!(
            history.entries[0].environment_path,
            Some(before.join("environment.toml"))
        );

        std::fs::rename(&before, &after).unwrap();
        history
            .relocate_environment(&before, &after.join("environment.toml"))
            .unwrap();
        let restored = History::load(state).unwrap();
        assert_eq!(
            restored.entries[0].environment_path,
            Some(after.join("environment.toml"))
        );
    }
}
