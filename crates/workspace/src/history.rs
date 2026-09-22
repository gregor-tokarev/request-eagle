use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use request::HttpRequest;
use serde::{Deserialize, Serialize};

use crate::history_writer::{HistoryWrite, HistoryWriter};

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HistoryEntry {
    pub name: String,
    pub request: Arc<HttpRequest>,
    pub environment_path: Option<PathBuf>,
    pub sent_at: u64,
}

impl HistoryEntry {
    pub fn new(name: String, request: HttpRequest, environment_path: Option<PathBuf>) -> Self {
        Self {
            name,
            request: Arc::new(request),
            environment_path,
            sent_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

struct PendingClear {
    entries: Vec<Arc<HistoryEntry>>,
    revision: Option<u64>,
}

#[derive(Default)]
pub(crate) struct History {
    pub entries: Vec<Arc<HistoryEntry>>,
    writer: Option<HistoryWriter>,
    revision: u64,
    pending_clear: Option<PendingClear>,
}

impl History {
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let entries = match fs::read(&path) {
            Ok(data) => {
                serde_json::from_slice::<Vec<Arc<HistoryEntry>>>(&data).map_err(io::Error::other)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };

        Ok(Self {
            entries: entries.into_iter().take(HISTORY_LIMIT).collect(),
            writer: Some(HistoryWriter::new(path)),
            ..Default::default()
        })
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        self.commit_persisted_clear();
        self.entries.insert(0, Arc::new(entry));
        self.entries.truncate(HISTORY_LIMIT);
    }

    pub fn relocate_environment(&mut self, previous_collection: &Path, environment: &Path) -> bool {
        self.commit_persisted_clear();
        let mut changed = false;

        // A failed Clear restores these entries. Keep their bindings current
        // while that write is pending, without copying their request bodies.
        for entry in self.entries.iter_mut().chain(
            self.pending_clear
                .iter_mut()
                .flat_map(|clear| clear.entries.iter_mut()),
        ) {
            if entry.environment_path.as_deref().and_then(Path::parent) == Some(previous_collection)
            {
                Arc::make_mut(entry).environment_path = Some(environment.to_path_buf());
                changed = true;
            }
        }

        changed
    }

    pub fn clear(&mut self) -> bool {
        self.commit_persisted_clear();

        if self.pending_clear.is_some() || self.entries.is_empty() {
            return false;
        }

        let entries = std::mem::take(&mut self.entries);
        if self.writer.is_some() {
            self.pending_clear = Some(PendingClear {
                entries,
                revision: None,
            });
        }

        true
    }

    pub fn is_clearing(&self) -> bool {
        self.pending_clear.is_some()
    }

    /// Queueing copies only Arc handles. Serialization and fsync belong to the
    /// returned background job, never to a Send, Clear, or rename callback.
    pub fn checkpoint(&mut self) -> Option<HistoryWrite> {
        self.commit_persisted_clear();
        let write = self.writer.as_ref()?.checkpoint(self.entries.clone());
        self.revision = write.revision();

        if let Some(clear) = &mut self.pending_clear {
            clear.revision.get_or_insert(self.revision);
        }

        Some(write)
    }

    /// Ignore outdated completion messages. A failed latest Clear restores
    /// the old entries after any newer attempts, keeping both sets of work.
    pub fn finish_write(&mut self, revision: u64, succeeded: bool) -> bool {
        self.commit_persisted_clear();

        if revision != self.revision {
            return false;
        }

        if !succeeded && let Some(clear) = self.pending_clear.take() {
            self.entries.extend(clear.entries);
            self.entries.truncate(HISTORY_LIMIT);
        }

        true
    }

    fn commit_persisted_clear(&mut self) {
        if let (Some(writer), Some(clear)) = (&self.writer, &self.pending_clear)
            && clear
                .revision
                .is_some_and(|revision| writer.committed_revision() >= revision)
        {
            self.pending_clear = None;
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let Some(write) = self.checkpoint() else {
            return Ok(());
        };
        let revision = write.revision();
        let result = write.write();
        self.finish_write(revision, result.is_ok());

        result
    }
}
