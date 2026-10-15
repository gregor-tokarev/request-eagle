use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use environment::Environment;

use super::links;
use crate::{Collection, Entry, FileEntry};

pub(crate) const ENVIRONMENT_FILE_NAME: &str = "environment.toml";

#[derive(Default)]
pub struct CollectionRegistry {
    pub(super) collections: Vec<Collection>,
    pub(super) directory: Option<PathBuf>,
    /// Folders outside `directory` linked as collections, such as a project's
    /// Git repository, in the order they were linked. A folder that is gone
    /// stays linked, as one on an unplugged drive comes back.
    pub(super) linked: Vec<PathBuf>,
    /// The file that lists the linked folders on this device, beside
    /// `directory`. Without one, as when it cannot be read, nothing links.
    pub(super) links_file: Option<PathBuf>,
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
                // Hidden folders, such as `.git`, are not collections.
                Ok(entry) if !is_hidden(&entry.path()) => collection_paths.push(entry.path()),
                Ok(_) => {}
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

            if let Some(collection) = load_collection(&collection_path, &mut registry.skipped) {
                registry.collections.push(collection);
            }
        }

        registry
    }

    /// Loads every collection in `directory`, then the folders linked as
    /// collections from elsewhere, which `links_file` lists. A linked folder
    /// that is gone, as on an unplugged drive, is left out until it is back.
    pub fn open(directory: impl AsRef<Path>, links_file: impl AsRef<Path>) -> Self {
        let mut registry = Self::from_path(directory);
        let links_file = links_file.as_ref();

        match links::read(links_file) {
            Ok(linked) => {
                registry.linked = linked;
                registry.links_file = Some(links_file.to_path_buf());
            }
            // Left as it is, so that linking cannot replace the list.
            Err(error) => registry.skipped.push(SkippedPath::new(links_file, &error)),
        }

        for path in registry.linked.clone() {
            if !path.exists() {
                continue;
            }

            if let Some(collection) = load_collection(&path, &mut registry.skipped) {
                registry.collections.push(collection);
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

    /// Whether a linked folder that was gone when the collections were
    /// loaded is back.
    pub fn linked_folder_returned(&self) -> bool {
        self.linked.iter().any(|path| {
            path.exists()
                && !self
                    .collections
                    .iter()
                    .any(|collection| collection.path == *path)
        })
    }

    /// Whether the collection at `path` is a folder linked from outside the
    /// collections directory.
    pub fn is_linked(&self, path: &Path) -> bool {
        self.linked.iter().any(|linked| linked == path)
    }

    pub fn file(&self, path: &Path) -> Option<&FileEntry> {
        match self.entry(path)? {
            Entry::File(file) => Some(file),
            _ => None,
        }
    }

    pub(crate) fn entry(&self, path: &Path) -> Option<&Entry> {
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

/// Loads the collection at `path`, or adds why it cannot be loaded to
/// `skipped`.
fn load_collection(path: &Path, skipped: &mut Vec<SkippedPath>) -> Option<Collection> {
    let environment_path = path.join(ENVIRONMENT_FILE_NAME);
    let environment = match Environment::from_file(&environment_path) {
        Ok(environment) => environment,
        Err(error) => {
            skipped.push(SkippedPath::new(&environment_path, &error));
            return None;
        }
    };

    match Collection::from_path(path, environment, skipped) {
        Ok(collection) => Some(collection),
        Err(error) => {
            skipped.push(SkippedPath::new(error.path(), &error));
            None
        }
    }
}

/// Whether a file or folder is hidden by its name, like `.git`. Loading
/// leaves hidden folders out.
pub(crate) fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
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
