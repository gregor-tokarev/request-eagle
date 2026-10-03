use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui_kit::{Context, EventEmitter};
use request::Request;

use crate::location::directory_name;
use crate::{
    Collection, CollectionEditError, CollectionRegistry, Entry, FileEntry, MovePlacement,
    SavedLocation, SharedSettings,
};

/// The saved collections, which the sidebar shows and tabs open. Every change
/// to their files goes through here, and `CollectionsEvent` tells whoever
/// shows them.
pub struct Collections {
    registry: CollectionRegistry,
    /// Counts the changes, so a view can tell whether it shows the latest.
    revision: u64,
}

/// A change to the saved collections, emitted after it is made.
pub enum CollectionsEvent {
    /// A collection, folder or request was created or imported.
    Created(PathBuf),
    /// A saved request's content changed.
    RequestSaved(PathBuf),
    CollectionRenamed {
        previous_path: PathBuf,
        path: PathBuf,
    },
    /// A folder was renamed or moved.
    FolderRelocated {
        previous_path: PathBuf,
        path: PathBuf,
        /// The directory of the collection that holds it now.
        collection: PathBuf,
    },
    /// A request was renamed or moved, or the collection or folder that holds
    /// it was. Follows the event of that collection or folder.
    RequestRelocated {
        previous_path: PathBuf,
        location: SavedLocation,
    },
    /// A collection, folder or request was deleted with everything in it.
    Deleted(PathBuf),
}

/// A collection or folder that can hold a new request.
#[derive(Clone)]
pub struct SaveDestination {
    pub path: PathBuf,
    /// The directory of the collection it is in, or is.
    pub collection: PathBuf,
}

impl SaveDestination {
    pub fn name(&self) -> String {
        directory_name(&self.path)
    }

    pub fn is_collection(&self) -> bool {
        self.path == self.collection
    }
}

impl EventEmitter<CollectionsEvent> for Collections {}

impl Collections {
    pub fn new(registry: CollectionRegistry) -> Self {
        Self {
            registry,
            revision: 0,
        }
    }

    pub fn registry(&self) -> &CollectionRegistry {
        &self.registry
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn collection(&self, path: &Path) -> Option<&Collection> {
        self.registry
            .collections()
            .iter()
            .find(|collection| collection.path == path)
    }

    /// The collection that is or holds `path`.
    pub fn containing(&self, path: &Path) -> Option<&Collection> {
        self.registry
            .collections()
            .iter()
            .find(|collection| path.starts_with(&collection.path))
    }

    /// The saved request at `path`, and where it is stored.
    pub fn request(&self, path: &Path) -> Option<(SavedLocation, &FileEntry)> {
        let file = self.registry.file(path)?;

        Some((self.location(file)?, file))
    }

    /// The saved requests in the collection or folder at `path`, in the order
    /// the sidebar shows them.
    pub fn requests_in(&self, path: &Path) -> Option<Vec<(SavedLocation, &FileEntry)>> {
        let collection = self.containing(path)?;
        let entries = if collection.path == path {
            &collection.entries
        } else {
            let Some(Entry::Directory(folder)) = self.registry.entry(path) else {
                return None;
            };
            &folder.entries
        };

        let mut files = Vec::new();
        add_files(entries, &mut files);

        Some(
            files
                .into_iter()
                .map(|file| (located(file, collection), file))
                .collect(),
        )
    }

    /// Where a saved request is stored.
    pub fn location(&self, file: &FileEntry) -> Option<SavedLocation> {
        Some(located(file, self.containing(&file.path)?))
    }

    /// Every collection and folder, in the order the sidebar shows them.
    pub fn save_destinations(&self) -> Vec<SaveDestination> {
        let mut destinations = Vec::new();
        for collection in self.registry.collections() {
            destinations.push(SaveDestination {
                path: collection.path.clone(),
                collection: collection.path.clone(),
            });
            add_folders(&collection.entries, &collection.path, &mut destinations);
        }

        destinations
    }

    pub fn create_collection(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.registry.create_collection()?;
        self.created(&path, cx);

        Ok(path)
    }

    pub fn create_folder(
        &mut self,
        parent: &Path,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.registry.create_folder(parent)?;
        self.created(&path, cx);

        Ok(path)
    }

    /// Save a new empty HTTP request in a collection or folder.
    pub fn create_request(
        &mut self,
        parent: &Path,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.registry.create_request(parent)?;
        self.created(&path, cx);

        Ok(path)
    }

    pub fn create_request_with(
        &mut self,
        parent: &Path,
        name: &str,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.registry.create_request_with(parent, name, request)?;
        self.created(&path, cx);

        Ok(path)
    }

    /// Add collections written elsewhere, such as by an import.
    pub fn add_collections(&mut self, collections: Vec<Collection>, cx: &mut Context<Self>) {
        let paths: Vec<_> = collections
            .iter()
            .map(|collection| collection.path.clone())
            .collect();
        for collection in collections {
            self.registry.add_collection(collection);
        }

        self.changed(cx);
        for path in paths {
            cx.emit(CollectionsEvent::Created(path));
        }
    }

    /// Save a request's changes, unless its file now holds another request.
    pub fn update_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Result<(), CollectionEditError> {
        self.registry.update_request(path, expected_id, request)?;
        self.changed(cx);
        cx.emit(CollectionsEvent::RequestSaved(path.to_path_buf()));

        Ok(())
    }

    /// Save a collection page's edits, renaming the collection's directory
    /// when its name changed. Returns the collection's path after the save.
    pub fn save_collection(
        &mut self,
        path: &Path,
        name: &str,
        variables: HashMap<String, String>,
        shared: SharedSettings,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        // Rename first: an invalid or taken name then fails before any file
        // changes. The rename event keeps the tab in step if a later write fails.
        let path = if path.file_name().is_some_and(|current| current == name) {
            path.to_path_buf()
        } else {
            self.rename(path, name, cx)?
        };
        self.registry.update_collection(&path, variables, shared)?;

        Ok(path)
    }

    /// Rename a collection, folder or request. Returns its path after the
    /// rename: collection and folder names are directory names, while a
    /// request's name is saved in its file.
    pub fn rename(
        &mut self,
        path: &Path,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let destination = self.registry.rename(path, name)?;
        self.relocated(path, &destination, cx);

        Ok(destination)
    }

    /// Rename a saved request, unless its file now holds another request.
    pub fn rename_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), CollectionEditError> {
        self.registry.rename_request(path, expected_id, name)?;
        self.relocated(path, path, cx);

        Ok(())
    }

    pub fn move_entry(
        &mut self,
        source: &Path,
        target: &Path,
        placement: MovePlacement,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let destination = self.registry.move_entry(source, target, placement)?;
        self.relocated(source, &destination, cx);

        Ok(destination)
    }

    pub fn delete(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<(), CollectionEditError> {
        self.registry.delete(path)?;
        self.changed(cx);
        cx.emit(CollectionsEvent::Deleted(path.to_path_buf()));

        Ok(())
    }

    fn created(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.changed(cx);
        cx.emit(CollectionsEvent::Created(path.to_path_buf()));
    }

    /// Report a collection, folder or request that moved from `previous` to
    /// `path`, or was renamed in place, followed by each request it holds.
    fn relocated(&mut self, previous: &Path, path: &Path, cx: &mut Context<Self>) {
        self.changed(cx);
        let Some(collection) = self.containing(path) else {
            return;
        };

        if collection.path == path {
            cx.emit(CollectionsEvent::CollectionRenamed {
                previous_path: previous.to_path_buf(),
                path: path.to_path_buf(),
            });
        } else if let Some(Entry::Directory(_)) = self.registry.entry(path) {
            cx.emit(CollectionsEvent::FolderRelocated {
                previous_path: previous.to_path_buf(),
                path: path.to_path_buf(),
                collection: collection.path.clone(),
            });
        }

        let requests = match self.request(path) {
            Some(request) => vec![request],
            None => self.requests_in(path).unwrap_or_default(),
        };
        for (location, _) in requests {
            let relative = location.path.strip_prefix(path).unwrap_or(Path::new(""));
            cx.emit(CollectionsEvent::RequestRelocated {
                // Joining an empty path would add a trailing separator.
                previous_path: if relative.as_os_str().is_empty() {
                    previous.to_path_buf()
                } else {
                    previous.join(relative)
                },
                location,
            });
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        cx.notify();
    }
}

fn located(file: &FileEntry, collection: &Collection) -> SavedLocation {
    SavedLocation {
        path: file.path.clone(),
        id: file.id.clone(),
        name: file.name.clone(),
        collection: collection.path.clone(),
    }
}

fn add_files<'a>(entries: &'a [Entry], files: &mut Vec<&'a FileEntry>) {
    for entry in entries {
        match entry {
            Entry::File(file) => files.push(file),
            Entry::Directory(folder) => add_files(&folder.entries, files),
        }
    }
}

fn add_folders(entries: &[Entry], collection: &Path, destinations: &mut Vec<SaveDestination>) {
    for entry in entries {
        if let Entry::Directory(folder) = entry {
            destinations.push(SaveDestination {
                path: folder.path.clone(),
                collection: collection.to_path_buf(),
            });
            add_folders(&folder.entries, collection, destinations);
        }
    }
}
