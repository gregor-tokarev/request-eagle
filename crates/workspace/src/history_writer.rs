use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{history::HistoryEntry, session::write_private_json};

#[derive(Clone)]
pub(crate) struct HistoryWriter {
    shared: Arc<HistoryWriterState>,
}

struct HistoryWriterState {
    path: PathBuf,
    revision: AtomicU64,
    committed_revision: AtomicU64,
    write_lock: Mutex<()>,
}

pub(crate) struct HistoryWrite {
    writer: HistoryWriter,
    revision: u64,
    entries: Vec<Arc<HistoryEntry>>,
}

impl HistoryWriter {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            shared: Arc::new(HistoryWriterState {
                path,
                revision: AtomicU64::new(0),
                committed_revision: AtomicU64::new(0),
                write_lock: Mutex::new(()),
            }),
        }
    }

    /// Queueing history never waits for a background disk write.
    pub(crate) fn checkpoint(&self, entries: Vec<Arc<HistoryEntry>>) -> HistoryWrite {
        HistoryWrite {
            writer: self.clone(),
            revision: self.shared.revision.fetch_add(1, Ordering::SeqCst) + 1,
            entries,
        }
    }

    #[cfg(test)]
    pub(crate) fn flush(&self, entries: Vec<Arc<HistoryEntry>>) -> io::Result<()> {
        self.checkpoint(entries).write()
    }

    pub(crate) fn committed_revision(&self) -> u64 {
        self.shared.committed_revision.load(Ordering::SeqCst)
    }
}

impl HistoryWrite {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Run on a background executor, except for the final synchronous flush.
    pub(crate) fn write(self) -> io::Result<()> {
        let shared = &self.writer.shared;
        let _write = shared
            .write_lock
            .lock()
            .map_err(|_| io::Error::other("History writer lock is poisoned"))?;

        if shared.revision.load(Ordering::SeqCst) != self.revision {
            return Ok(());
        }

        write_private_json(&shared.path, &self.entries)?;
        shared
            .committed_revision
            .store(self.revision, Ordering::SeqCst);

        Ok(())
    }
}
