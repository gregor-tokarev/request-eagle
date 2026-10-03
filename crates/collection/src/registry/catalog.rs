use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use environment::Environment;

use crate::{Collection, Entry, FileEntry};

pub(super) const ENVIRONMENT_FILE_NAME: &str = "environment.toml";

#[derive(Default)]
pub struct CollectionRegistry {
    pub(super) collections: Vec<Collection>,
    pub(super) directory: Option<PathBuf>,
    pub(super) skipped: Vec<SkippedPath>,
}

/// A file or folder that could not be loaded. It is left out and stays on
/// disk as it is. A collection whose environment or settings cannot be read
/// is left out whole, so that saving it cannot replace them.
#[derive(Debug)]
pub struct SkippedPath {
    pub path: PathBuf,
    pub error: String,
}

impl SkippedPath {
    pub(crate) fn new(path: &Path, error: &dyn Error) -> Self {
        Self {
            path: path.to_path_buf(),
            // The error's own description repeats the path.
            error: error.source().unwrap_or(error).to_string(),
        }
    }
}

impl CollectionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every collection in `path`. A missing directory has none.
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let mut registry = Self {
            directory: Some(path.to_path_buf()),
            ..Self::new()
        };
        let directory = match fs::read_dir(path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return registry,
            Err(error) => {
                registry.skipped.push(SkippedPath::new(path, &error));
                return registry;
            }
        };

        let mut collection_paths = Vec::new();
        for entry in directory {
            match entry {
                Ok(entry) => collection_paths.push(entry.path()),
                Err(error) => registry.skipped.push(SkippedPath::new(path, &error)),
            }
        }
        collection_paths.sort();

        for collection_path in collection_paths {
            match fs::metadata(&collection_path) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => continue,
                Err(error) => {
                    registry
                        .skipped
                        .push(SkippedPath::new(&collection_path, &error));
                    continue;
                }
            }

            let environment_path = collection_path.join(ENVIRONMENT_FILE_NAME);
            let environment = match Environment::from_file(&environment_path) {
                Ok(environment) => environment,
                Err(error) => {
                    registry
                        .skipped
                        .push(SkippedPath::new(&environment_path, &error));
                    continue;
                }
            };

            match Collection::from_path(&collection_path, environment, &mut registry.skipped) {
                Ok(collection) => registry.collections.push(collection),
                Err(error) => registry
                    .skipped
                    .push(SkippedPath::new(error.path(), &error)),
            }
        }

        registry
    }

    /// What loading left out, in the order it was found.
    pub fn skipped(&self) -> &[SkippedPath] {
        &self.skipped
    }

    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    /// Where new collections are created.
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    pub fn file(&self, path: &Path) -> Option<&FileEntry> {
        match self.entry(path)? {
            Entry::File(file) => Some(file),
            _ => None,
        }
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.collections.iter().find_map(|collection| {
            path.starts_with(&collection.path)
                .then(|| find(&collection.entries, path))
                .flatten()
        })
    }

    /// The saved request with this ID and the collection that holds it.
    /// Flows refer to requests by ID, which moving or renaming keeps.
    pub fn request_by_id(&self, id: &str) -> Option<(&Collection, &FileEntry)> {
        self.collections.iter().find_map(|collection| {
            find_by_id(&collection.entries, id).map(|file| (collection, file))
        })
    }
}

fn find<'a>(entries: &'a [Entry], path: &Path) -> Option<&'a Entry> {
    entries.iter().find_map(|entry| match entry {
        Entry::Directory(folder) if folder.path == path => Some(entry),
        Entry::Directory(folder) if path.starts_with(&folder.path) => find(&folder.entries, path),
        entry if entry.path() == path => Some(entry),
        _ => None,
    })
}

fn find_by_id<'a>(entries: &'a [Entry], id: &str) -> Option<&'a FileEntry> {
    entries.iter().find_map(|entry| match entry {
        Entry::File(file) if file.id == id => Some(file),
        Entry::Directory(folder) => find_by_id(&folder.entries, id),
        _ => None,
    })
}
