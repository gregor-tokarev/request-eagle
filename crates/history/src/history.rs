use std::{
    fs, io,
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::Record;

/// The most requests history keeps. The oldest are deleted as new ones arrive.
pub const LIMIT: usize = 500;

const INDEX: &str = "index.json";
const ENTRIES: &str = "entries";

/// One sent request, as history lists it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Milliseconds since the Unix epoch.
    pub sent_at: u64,
    /// The HTTP method or protocol.
    pub label: String,
    /// Where the request went, as written.
    pub address: String,
}

impl Entry {
    pub fn sent_at(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.sent_at)
    }
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("{0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("the request is no longer in history")]
    Missing,
}

/// Sent requests, newest first. The list is one small file, read when the
/// app opens; each request and its response are files of their own, read
/// when the request is opened.
///
/// Changes apply to the list at once. Each returns the [`Change`] that saves
/// it, so the files can be written off the UI thread.
pub struct History {
    directory: PathBuf,
    entries: Vec<Entry>,
}

impl History {
    /// An empty history stored in `directory`. Call [`History::load`] to read it.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            entries: Vec::new(),
        }
    }

    /// Read the list of sent requests. A missing list is empty.
    pub fn load(&mut self) -> Result<(), HistoryError> {
        self.entries = match fs::read(self.directory.join(INDEX)) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };

        Ok(())
    }

    /// Newest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Keep a sent request, deleting the oldest beyond [`LIMIT`].
    pub fn add(&mut self, record: Record, sent_at: SystemTime) -> Change {
        let entry = Entry {
            id: Uuid::new_v4().to_string(),
            sent_at: sent_at
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            label: record.label().to_owned(),
            address: record.address(),
        };
        let id = entry.id.clone();

        // A slow request can complete after one sent later.
        let position = self
            .entries
            .partition_point(|newer| newer.sent_at > entry.sent_at);
        self.entries.insert(position, entry);

        let removed = self
            .entries
            .split_off(LIMIT.min(self.entries.len()))
            .into_iter()
            .map(|old| old.id)
            .collect();

        Change {
            added: Some((id, record)),
            removed,
            ..self.change()
        }
    }

    pub fn delete(&mut self, id: &str) -> Change {
        let removed = match self.entries.iter().position(|entry| entry.id == id) {
            Some(position) => vec![self.entries.remove(position).id],
            None => Vec::new(),
        };

        Change {
            removed,
            ..self.change()
        }
    }

    pub fn clear(&mut self) -> Change {
        self.entries.clear();

        Change {
            cleared: true,
            ..self.change()
        }
    }

    /// The files of a listed entry's request and response.
    pub fn files(&self, id: &str) -> Option<RecordFiles> {
        if !self.entries.iter().any(|entry| entry.id == id) || Uuid::parse_str(id).is_err() {
            return None;
        }

        let entries = self.directory.join(ENTRIES);

        Some(RecordFiles {
            record: entries.join(format!("{id}.json")),
            body: entries.join(format!("{id}.body")),
        })
    }

    /// A change that saves the list as it is now.
    fn change(&self) -> Change {
        Change {
            directory: self.directory.clone(),
            added: None,
            removed: Vec::new(),
            cleared: false,
            index: self.entries.clone(),
        }
    }
}

/// The files to write after history changed. Save changes in the order they
/// were made.
#[must_use]
pub struct Change {
    directory: PathBuf,
    added: Option<(String, Record)>,
    removed: Vec<String>,
    cleared: bool,
    index: Vec<Entry>,
}

impl Change {
    /// The id of the request this change adds.
    pub fn added(&self) -> Option<&str> {
        self.added.as_ref().map(|(id, _)| id.as_str())
    }

    pub fn save(self) -> Result<(), HistoryError> {
        let entries = self.directory.join(ENTRIES);

        if self.cleared {
            for result in [
                fs::remove_dir_all(&entries),
                fs::remove_file(self.directory.join(INDEX)),
            ] {
                if let Err(error) = result
                    && error.kind() != io::ErrorKind::NotFound
                {
                    return Err(error.into());
                }
            }

            return Ok(());
        }

        if let Some((id, record)) = &self.added {
            if let Some(response) = &record.response
                && let Some(body) = &response.body
            {
                write(&entries.join(format!("{id}.body")), body)?;
            }

            write(
                &entries.join(format!("{id}.json")),
                &serde_json::to_vec(record)?,
            )?;
        }

        for id in &self.removed {
            for extension in ["json", "body"] {
                if let Err(error) = fs::remove_file(entries.join(format!("{id}.{extension}")))
                    && error.kind() != io::ErrorKind::NotFound
                {
                    return Err(error.into());
                }
            }
        }

        write(
            &self.directory.join(INDEX),
            &serde_json::to_vec(&self.index)?,
        )
    }
}

/// Where an entry's request and response are kept.
pub struct RecordFiles {
    record: PathBuf,
    body: PathBuf,
}

impl RecordFiles {
    pub fn read(&self) -> Result<Record, HistoryError> {
        let mut record: Record = match fs::read(&self.record) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(HistoryError::Missing);
            }
            Err(error) => return Err(error.into()),
        };

        if let Some(response) = &mut record.response {
            response.body = match fs::read(&self.body) {
                Ok(body) => Some(body),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
        }

        Ok(record)
    }
}

/// Replace a file whole, so an interrupted write leaves the previous one.
fn write(path: &Path, contents: &[u8]) -> Result<(), HistoryError> {
    let directory = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(directory)?;

    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(contents)?;
    file.persist(path).map_err(|error| error.error)?;

    Ok(())
}
