use std::{
    collections::HashMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

use environment::Environment;

use super::catalog::{ENVIRONMENT_FILE_NAME, is_hidden};
use super::links;
use super::mutations::request_changed;
use crate::collection::{SETTINGS_FILE_NAME, load_file};
use crate::{Collection, CollectionEditError, CollectionRegistry, Entry, FileEntry, SkippedPath};

/// A folder outside the collections directory, read to be linked as a
/// collection with [`CollectionRegistry::link`].
pub struct LinkedFolder {
    collection: Collection,
    skipped: Vec<SkippedPath>,
}

impl LinkedFolder {
    /// Reads the collection in the folder at `path`. The folder must be
    /// empty or be a collection itself, with a request, its variables, its
    /// settings or its order at the top. A project that holds a collection
    /// deeper down is not one, so that the project's own files never show up
    /// as requests and folders, or get committed with them.
    ///
    /// A large folder takes a while to read, so this works without the
    /// registry and can run in the background.
    pub fn read(path: &Path) -> Result<Self, CollectionEditError> {
        let path = normalized(path)?;
        if !is_empty(&path)? && !is_collection(&path)? {
            return Err(CollectionEditError::NotACollection);
        }

        let environment = Environment::from_file(path.join(ENVIRONMENT_FILE_NAME))?;
        let mut skipped = Vec::new();
        let collection = Collection::from_path(&path, environment, &mut skipped)?;

        Ok(Self {
            collection,
            skipped,
        })
    }

    pub fn path(&self) -> &Path {
        &self.collection.path
    }
}

impl CollectionRegistry {
    /// Adds a folder read with [`LinkedFolder::read`] as a collection and
    /// remembers it on this device. Returns the collection's path.
    pub fn link(&mut self, folder: LinkedFolder) -> Result<PathBuf, CollectionEditError> {
        let path = folder.collection.path.clone();
        let overlaps = |other: &Path| path.starts_with(other) || other.starts_with(&path);
        if self
            .collections
            .iter()
            .any(|collection| overlaps(&collection.path))
            || self.directory.as_deref().is_some_and(overlaps)
        {
            return Err(CollectionEditError::AlreadyCollection);
        }

        // A folder linked before that could not be loaded keeps its place.
        if !self.is_linked(&path) {
            self.update_links(|linked| linked.push(path.clone()))?;
        }

        self.skipped
            .retain(|skipped| !skipped.path.starts_with(&path));
        self.skipped.extend(folder.skipped);
        self.collections.push(folder.collection);

        Ok(path)
    }

    /// Stops showing a linked collection. Its folder stays as it is.
    pub fn unlink(&mut self, path: &Path) -> Result<(), CollectionEditError> {
        let path = normalized(path)?;
        if !self.is_linked(&path) {
            return Err(CollectionEditError::NotFound);
        }

        self.update_links(|linked| linked.retain(|linked| *linked != path))?;
        self.collections
            .retain(|collection| collection.path != path);
        self.skipped
            .retain(|skipped| !skipped.path.starts_with(&path));

        Ok(())
    }

    /// Changes the list of linked folders. The list is read again first, so
    /// that a change made meanwhile, such as by the CLI, stays, and a list
    /// that cannot be read is never replaced.
    pub(super) fn update_links(
        &mut self,
        change: impl FnOnce(&mut Vec<PathBuf>),
    ) -> io::Result<()> {
        let links_file = self
            .links_file
            .as_deref()
            .ok_or_else(|| io::Error::other("Linked collections are not available here."))?;

        let mut linked = links::read(links_file)?;
        change(&mut linked);
        links::write(links_file, &linked)?;
        self.linked = linked;

        Ok(())
    }

    /// Reads every collection again, such as after Git changed their files.
    /// A request whose file changed counts another change from outside the
    /// app, so a tab opened from it before can tell.
    pub fn reload(&mut self) {
        let Some(directory) = self.directory.clone() else {
            return;
        };

        let mut reloaded = match &self.links_file {
            Some(links_file) => Self::open(&directory, links_file),
            None => Self::from_path(&directory),
        };

        let mut known = HashMap::new();
        for collection in &self.collections {
            index_files(&collection.entries, &mut known);
        }
        for collection in &mut reloaded.collections {
            carry_outside_changes(&mut collection.entries, &known);
        }

        *self = reloaded;
    }
}

fn index_files<'a>(entries: &'a [Entry], files: &mut HashMap<&'a Path, &'a FileEntry>) {
    for entry in entries {
        match entry {
            Entry::File(file) => {
                files.insert(&file.path, file);
            }
            Entry::Directory(folder) => index_files(&folder.entries, files),
        }
    }
}

fn carry_outside_changes(entries: &mut [Entry], known: &HashMap<&Path, &FileEntry>) {
    for entry in entries {
        match entry {
            Entry::File(file) => {
                if let Some(known) = known
                    .get(file.path.as_path())
                    .filter(|known| known.id == file.id)
                {
                    file.outside_changes =
                        known.outside_changes + u64::from(request_changed(known, file));
                }
            }
            Entry::Directory(folder) => carry_outside_changes(&mut folder.entries, known),
        }
    }
}

/// Whether a folder is a collection by what it holds at its top: a request,
/// or the collection's variables, settings or order.
fn is_collection(path: &Path) -> io::Result<bool> {
    let markers = [
        path.join(ENVIRONMENT_FILE_NAME),
        path.join(SETTINGS_FILE_NAME),
        path.join(crate::order::FILE_NAME),
    ];
    if markers.iter().any(|marker| marker.exists()) {
        return Ok(true);
    }

    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "toml")
            && load_file(&path).is_ok()
        {
            return Ok(true);
        }
    }

    Ok(false)
}

/// `path` made absolute, without `.` and `..`, so that a folder has one
/// path however it was reached.
fn normalized(path: &Path) -> io::Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component),
        }
    }

    Ok(normalized)
}

/// Whether a folder holds nothing but hidden items, such as a new Git
/// repository.
fn is_empty(path: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(path)? {
        if !is_hidden(&entry?.path()) {
            return Ok(false);
        }
    }

    Ok(true)
}
