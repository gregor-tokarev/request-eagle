use std::path::{Path, PathBuf};

use request::Request;
use serde::{Deserialize, Serialize};

// Most entries are requests, so boxing them would not make trees smaller.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Entry {
    File(FileEntry),
    Directory(DirEntry),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileEntry {
    #[serde(skip)]
    pub(crate) raw_content: String,
    /// How many times the request was taken from its file after it changed
    /// outside the app. A tab keeps the count it opened with, and a later
    /// count tells it that it edits an older request than this one.
    #[serde(skip)]
    pub(crate) outside_changes: u64,
    #[serde(skip)]
    pub path: PathBuf,

    pub id: String,
    pub name: String,
    pub schema_version: u8,

    pub request: Request,
}

#[derive(Debug)]
pub struct DirEntry {
    pub path: PathBuf,
    pub name: String,
    pub entries: Vec<Entry>,
}

impl FileEntry {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, crate::CollectionLoadError> {
        crate::collection::load_file(path.as_ref())
    }

    pub fn outside_changes(&self) -> u64 {
        self.outside_changes
    }
}

impl Entry {
    pub fn path(&self) -> &Path {
        match self {
            Self::File(file) => &file.path,
            Self::Directory(folder) => &folder.path,
        }
    }
}
