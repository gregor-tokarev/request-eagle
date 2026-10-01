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
    pub fn add(&mut self, record: &Record, sent_at: SystemTime) -> Result<(), HistoryError> {
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

        let entries = self.directory.join(ENTRIES);
        fs::create_dir_all(&entries)?;
        if let Some(response) = &record.response
            && let Some(body) = &response.body
        {
            write(&entries.join(format!("{}.body", entry.id)), body)?;
        }
        write(
            &entries.join(format!("{}.json", entry.id)),
            &serde_json::to_vec(record)?,
        )?;

        // A slow request can complete after one sent later.
        let position = self
            .entries
            .partition_point(|newer| newer.sent_at > entry.sent_at);
        self.entries.insert(position, entry);

        for old in self.entries.split_off(LIMIT.min(self.entries.len())) {
            remove_files(&entries, &old.id)?;
        }

        self.save()
    }

    /// The request and response kept for an entry.
    pub fn read(&self, id: &str) -> Result<Record, HistoryError> {
        if !self.entries.iter().any(|entry| entry.id == id) || Uuid::parse_str(id).is_err() {
            return Err(HistoryError::Missing);
        }

        let entries = self.directory.join(ENTRIES);
        let mut record: Record = match fs::read(entries.join(format!("{id}.json"))) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(HistoryError::Missing);
            }
            Err(error) => return Err(error.into()),
        };

        if let Some(response) = &mut record.response {
            response.body = match fs::read(entries.join(format!("{id}.body"))) {
                Ok(body) => Some(body),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
        }

        Ok(record)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), HistoryError> {
        let Some(position) = self.entries.iter().position(|entry| entry.id == id) else {
            return Ok(());
        };

        let entry = self.entries.remove(position);
        remove_files(&self.directory.join(ENTRIES), &entry.id)?;

        self.save()
    }

    pub fn clear(&mut self) -> Result<(), HistoryError> {
        self.entries.clear();

        for result in [
            fs::remove_dir_all(self.directory.join(ENTRIES)),
            fs::remove_file(self.directory.join(INDEX)),
        ] {
            if let Err(error) = result
                && error.kind() != io::ErrorKind::NotFound
            {
                return Err(error.into());
            }
        }

        Ok(())
    }

    fn save(&self) -> Result<(), HistoryError> {
        write(
            &self.directory.join(INDEX),
            &serde_json::to_vec(&self.entries)?,
        )
    }
}

fn remove_files(entries: &Path, id: &str) -> io::Result<()> {
    for extension in ["json", "body"] {
        if let Err(error) = fs::remove_file(entries.join(format!("{id}.{extension}")))
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
    }

    Ok(())
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
