use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::collection::{load_file, save_file};
use crate::{CollectionLoadError, CollectionRegistry, CollectionSaveError, Entry};

impl CollectionRegistry {
    /// Request names live in TOML; collection and folder names live on disk.
    pub fn rename(&mut self, path: &Path, name: &str) -> Result<PathBuf, CollectionEditError> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(CollectionEditError::InvalidName);
        }

        for collection in &mut self.collections {
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
                        let mut updated = load_file(path)?;
                        updated.name = name.to_owned();
                        save_file(&mut updated)?;
                        *file = updated;
                        return Ok(path.to_path_buf());
                    }
                    Entry::Directory(folder) => {
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
            return Ok(());
        }

        for collection in &mut self.collections {
            if delete_entry(&mut collection.entries, path)? {
                return Ok(());
            }
        }

        Err(CollectionEditError::NotFound)
    }
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

            #[cfg(not(unix))]
            {
                let _ = destination_metadata;
                return Err(CollectionEditError::AlreadyExists);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    fs::rename(path, &destination)?;
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

fn rebase_entries(entries: &mut [Entry], old: &Path, new: &Path) {
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
        *path = new.join(relative);
    }
}

#[derive(Debug, Error)]
pub enum CollectionEditError {
    #[error("Enter a valid name without path separators or control characters.")]
    InvalidName,
    #[error("An item with that name already exists.")]
    AlreadyExists,
    #[error("This item is no longer in the collection.")]
    NotFound,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Load(#[from] CollectionLoadError),
    #[error("{0}")]
    Save(#[from] CollectionSaveError),
}
