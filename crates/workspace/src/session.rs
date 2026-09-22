use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use collection::HttpRequest;
use serde::{Deserialize, Serialize};

const SESSION_VERSION: u32 = 1;

/// Recovery contains the draft itself, even when it belongs to a saved request.
/// Reopening the collection file instead would discard unsaved edits.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RecoveredTab {
    pub(crate) title: String,
    pub(crate) name: String,
    pub(crate) collection: Option<String>,
    pub(crate) request_path: Option<PathBuf>,
    pub(crate) environment_path: Option<PathBuf>,
    pub(crate) request: HttpRequest,
    pub(crate) saved_request: Option<HttpRequest>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct SessionSnapshot {
    pub(crate) tabs: Vec<RecoveredTab>,
    pub(crate) selected: Option<usize>,
}

/// Cached tabs let the UI queue recovery without copying every request body.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct SessionCheckpoint {
    pub(crate) tabs: Vec<Arc<RecoveredTab>>,
    pub(crate) selected: Option<usize>,
}

#[derive(Deserialize, Serialize)]
struct SessionFile<S> {
    version: u32,
    session: S,
}

pub(crate) struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// A read error must disable autosaving until the user can recover the old
    /// file. Returning an empty session here would allow later edits to erase it.
    pub(crate) fn load(&self) -> io::Result<Option<SessionSnapshot>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let file: SessionFile<SessionSnapshot> = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        if file.version != SESSION_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported session recovery version {}", file.version),
            ));
        }

        validate_selected_tab(file.session.selected, file.session.tabs.len())?;

        Ok(Some(file.session))
    }

    #[cfg(test)]
    pub(crate) fn save(&self, session: &SessionSnapshot) -> io::Result<()> {
        validate_selected_tab(session.selected, session.tabs.len())?;

        write_private_json(
            &self.path,
            &SessionFile {
                version: SESSION_VERSION,
                session,
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct SessionWriter {
    shared: Arc<SessionWriterState>,
}

struct SessionWriterState {
    store: SessionStore,
    revision: AtomicU64,
    write_lock: Mutex<()>,
}

pub(crate) struct SessionWrite {
    writer: SessionWriter,
    revision: u64,
    checkpoint: SessionCheckpoint,
}

impl SessionWriter {
    pub(crate) fn new(store: SessionStore) -> Self {
        Self {
            shared: Arc::new(SessionWriterState {
                store,
                revision: AtomicU64::new(0),
                write_lock: Mutex::new(()),
            }),
        }
    }

    /// Preparing background work never waits for an in-progress disk write.
    pub(crate) fn checkpoint(&self, checkpoint: SessionCheckpoint) -> SessionWrite {
        SessionWrite {
            writer: self.clone(),
            revision: self.shared.revision.fetch_add(1, Ordering::SeqCst) + 1,
            checkpoint,
        }
    }

    /// Finish the newest snapshot before quitting. Any already queued older
    /// work will be skipped, even if its executor starts it after this returns.
    pub(crate) fn flush(&self, checkpoint: SessionCheckpoint) -> io::Result<()> {
        self.checkpoint(checkpoint).write()
    }
}

impl SessionWrite {
    /// Run on a background executor, except for the final synchronous flush.
    pub(crate) fn write(self) -> io::Result<()> {
        let shared = &self.writer.shared;
        let _write = shared
            .write_lock
            .lock()
            .map_err(|_| io::Error::other("Session writer lock is poisoned"))?;

        if shared.revision.load(Ordering::SeqCst) != self.revision {
            return Ok(());
        }

        validate_selected_tab(self.checkpoint.selected, self.checkpoint.tabs.len())?;

        write_private_json(
            &shared.store.path,
            &SessionFile {
                version: SESSION_VERSION,
                session: self.checkpoint,
            },
        )
    }
}

fn validate_selected_tab(selected: Option<usize>, tab_count: usize) -> io::Result<()> {
    if selected.is_some_and(|index| index >= tab_count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Recovered selected tab does not exist",
        ));
    }

    Ok(())
}

/// Session drafts and history can contain credentials. Create files with owner
/// access only and commit by rename so a crash cannot leave half-written JSON.
pub(crate) fn write_private_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;

        directory.mode(0o700);
    }

    directory.create(parent)?;

    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }

    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;

    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;

    Ok(())
}
