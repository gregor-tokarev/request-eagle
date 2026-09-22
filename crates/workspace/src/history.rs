use std::{
    fs, io,
    path::PathBuf,
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
