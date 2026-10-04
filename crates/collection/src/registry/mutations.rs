use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use environment::EnvironmentSaveError;
use thiserror::Error;

use crate::collection::{is_reserved, load_file, save_file};
use crate::{
    CollectionLoadError, CollectionRegistry, CollectionSaveError, Entry, FileEntry, SharedSettings,
};
use request::Request;

impl CollectionRegistry {
    /// Saves a request without replacing its identity or externally edited
    /// metadata. A request that changed in its file since it was last read
    /// here is not saved, so a change made outside the app is not lost.
    pub fn update_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
    ) -> Result<(), CollectionEditError> {
        self.save_request(path, expected_id, request, false)
    }

    /// Saves a request over the one its file holds now, once the user chose
    /// to keep their own changes. Externally edited metadata still stays.
    pub fn overwrite_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
    ) -> Result<(), CollectionEditError> {
        self.save_request(path, expected_id, request, true)
    }

    /// Reads a request's file again, taking the changes made to it outside
    /// the app.
    pub fn reload_request(
        &mut self,
        path: &Path,
        expected_id: &str,
    ) -> Result<(), CollectionEditError> {
        let file = self.file_mut(path, expected_id)?;
        let latest = load_file(path)?;
        if latest.id != expected_id {
            return Err(CollectionEditError::RequestReplaced);
        }

        *file = latest;

        Ok(())
    }

    /// Renames a request, unless its file now holds a different request.
    pub fn rename_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        name: &str,
    ) -> Result<(), CollectionEditError> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(CollectionEditError::InvalidName);
        }

        let file = self.file_mut(path, expected_id)?;
        let latest = load_file(path)?;
        if latest.id != expected_id {
            return Err(CollectionEditError::RequestReplaced);
        }

        rename_file(file, latest, name)
    }

    /// Saves a request in the latest content of its file, keeping comments
    /// and external edits, as long as it is still the expected request.
    /// Unless `overwrite` is set, a request changed outside the app stays.
    fn save_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
        overwrite: bool,
    ) -> Result<(), CollectionEditError> {
        let file = self.file_mut(path, expected_id)?;
        let mut updated = load_file(path)?;
        if updated.id != expected_id {
            return Err(CollectionEditError::RequestReplaced);
        }
        if !overwrite && request_changed(file, &updated) {
            return Err(CollectionEditError::ChangedOnDisk);
        }

        updated.request = request;
        save_file(&mut updated)?;
        *file = updated;

        Ok(())
    }

    /// The request at `path`, as long as it is still the expected one.
    fn file_mut(
        &mut self,
        path: &Path,
        expected_id: &str,
    ) -> Result<&mut FileEntry, CollectionEditError> {
        let file = self
            .collections
            .iter_mut()
            .find_map(
                |collection| match find_entry(&mut collection.entries, path) {
                    Some(Entry::File(file)) => Some(file),
                    _ => None,
                },
            )
            .ok_or(CollectionEditError::NotFound)?;

        if file.id != expected_id {
            return Err(CollectionEditError::RequestReplaced);
        }

        Ok(file)
    }

    /// Saves the collection's variables to `environment.toml` and its scripts
    /// and authorization to the settings file, leaving unchanged files as
    /// they are.
    pub fn update_collection(
        &mut self,
        path: &Path,
        variables: HashMap<String, String>,
        shared: SharedSettings,
    ) -> Result<(), CollectionEditError> {
        self.collections
            .iter_mut()
            .find(|collection| collection.path == path)
            .ok_or(CollectionEditError::NotFound)?
            .save_settings(variables, shared)
    }

    /// Request names live in TOML; collection and folder names live on disk.
    pub fn rename(&mut self, path: &Path, name: &str) -> Result<PathBuf, CollectionEditError> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(CollectionEditError::InvalidName);
        }

        for collection in &mut self.collections {
            let reserved = collection.reserved_paths();
            if collection.path == path {
                let destination = rename_directory(path, name)?;
                rebase_entries(&mut collection.entries, path, &destination);
                rebase_path(&mut collection.local_env.path, path, &destination);
                collection.path = destination.clone();
                return Ok(destination);
            }

            if let Some(entry) = find_entry(&mut collection.entries, path) {
                match entry {
                    Entry::File(file) => {
                        // Read the latest content so external edits and comments survive.
                        let latest = load_file(path)?;
                        rename_file(file, latest, name)?;
                        return Ok(path.to_path_buf());
                    }
                    Entry::Directory(folder) => {
                        if is_reserved(&reserved, &path.with_file_name(name)) {
                            return Err(CollectionEditError::ReservedName);
                        }
                        let destination = rename_directory(path, name)?;
                        rebase_entries(&mut folder.entries, path, &destination);
                        folder.path = destination.clone();
                        folder.name = name.to_owned();
                        return Ok(destination);
                    }
                }
            }
        }

        Err(CollectionEditError::NotFound)
    }

    pub fn delete(&mut self, path: &Path) -> Result<(), CollectionEditError> {
        if let Some(index) = self
            .collections
            .iter()
            .position(|collection| collection.path == path)
        {
            fs::remove_dir_all(path)?;
            self.collections.remove(index);
            self.skipped
                .retain(|skipped| !skipped.path.starts_with(path));
            return Ok(());
        }

        for collection in &mut self.collections {
            if delete_entry(&mut collection.entries, path)? {
                self.skipped
                    .retain(|skipped| !skipped.path.starts_with(path));
                return Ok(());
            }
        }

        Err(CollectionEditError::NotFound)
    }
}

/// Names a request in the `latest` content of its file. A request changed
/// outside the app stays in the file without becoming the known one: a tab
/// that edits the request was opened before the change, and saving it still
/// has to ask whose changes to keep.
fn rename_file(
    file: &mut FileEntry,
    mut latest: FileEntry,
    name: &str,
) -> Result<(), CollectionEditError> {
    let changed_outside = latest.id == file.id && request_changed(file, &latest);
    latest.name = name.to_owned();
    save_file(&mut latest)?;

    if changed_outside {
        file.name = latest.name;
    } else {
        *file = latest;
    }

    Ok(())
}

/// Whether the request in a file differs from the one last read from it.
/// Comments, the name and fields the app does not know are left out: saving
/// keeps them as the file has them.
fn request_changed(known: &FileEntry, latest: &FileEntry) -> bool {
    if latest.raw_content == known.raw_content {
        return false;
    }

    // Compared as the file held it, not as the editor handed it over to be
    // saved, in case a request reads back in another form than it was written.
    toml::from_str::<FileEntry>(&known.raw_content)
        .map_or(true, |known| known.request != latest.request)
}

fn rename_directory(path: &Path, name: &str) -> Result<PathBuf, CollectionEditError> {
    if name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(CollectionEditError::InvalidName);
    }

    let destination = path.with_file_name(name);
    if destination == path {
        return Ok(destination);
    }

    // Allow case-only renames on case-insensitive filesystems, but never
    // replace a different sibling, including an empty folder or symlink.
    match fs::symlink_metadata(&destination) {
        Ok(destination_metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;

                let source_metadata = fs::symlink_metadata(path)?;
                if source_metadata.dev() != destination_metadata.dev()
                    || source_metadata.ino() != destination_metadata.ino()
                {
                    return Err(CollectionEditError::AlreadyExists);
                }
            }

            // Both paths resolve to the folder's real name when only their
            // case differs.
            #[cfg(not(unix))]
            {
                let _ = destination_metadata;
                if fs::canonicalize(path)? != fs::canonicalize(&destination)? {
                    return Err(CollectionEditError::AlreadyExists);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    crate::order::rename_directory(path, &destination)?;
    Ok(destination)
}

pub(super) fn find_entry<'a>(entries: &'a mut [Entry], path: &Path) -> Option<&'a mut Entry> {
    for entry in entries {
        match entry {
            Entry::File(file) if file.path == path => return Some(entry),
            Entry::Directory(folder) if folder.path == path => return Some(entry),
            Entry::Directory(folder) => {
                if let Some(entry) = find_entry(&mut folder.entries, path) {
                    return Some(entry);
                }
            }
            _ => {}
        }
    }

    None
}

fn delete_entry(entries: &mut Vec<Entry>, path: &Path) -> Result<bool, io::Error> {
    for index in 0..entries.len() {
        match &mut entries[index] {
            Entry::File(file) if file.path == path => fs::remove_file(path)?,
            Entry::Directory(folder) if folder.path == path => fs::remove_dir_all(path)?,
            Entry::Directory(folder) => {
                if delete_entry(&mut folder.entries, path)? {
                    return Ok(true);
                }
                continue;
            }
            _ => continue,
        }

        entries.remove(index);
        return Ok(true);
    }

    Ok(false)
}

pub(super) fn rebase_entries(entries: &mut [Entry], old: &Path, new: &Path) {
    for entry in entries {
        match entry {
            Entry::File(file) => rebase_path(&mut file.path, old, new),
            Entry::Directory(folder) => {
                rebase_path(&mut folder.path, old, new);
                rebase_entries(&mut folder.entries, old, new);
            }
        }
    }
}

fn rebase_path(path: &mut PathBuf, old: &Path, new: &Path) {
    if let Ok(relative) = path.strip_prefix(old) {
        *path = if relative.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(relative)
        };
    }
}

#[derive(Debug, Error)]
pub enum CollectionEditError {
    #[error("Enter a valid name without path separators or control characters.")]
    InvalidName,
    #[error("An item with that name already exists.")]
    AlreadyExists,
    #[error("That name is reserved for the collection's settings.")]
    ReservedName,
    #[error("This item is no longer in the collection.")]
    NotFound,
    #[error("This request was replaced by a different request. Your edits have not been saved.")]
    RequestReplaced,
    #[error("This request was changed outside Request Eagle. Your edits have not been saved.")]
    ChangedOnDisk,
    #[error("A folder cannot be moved into itself or its descendants.")]
    InvalidMove,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Load(#[from] CollectionLoadError),
    #[error("{0}")]
    Save(#[from] CollectionSaveError),
    #[error("{0}")]
    Environment(#[from] EnvironmentSaveError),
}
