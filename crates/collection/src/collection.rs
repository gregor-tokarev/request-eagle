use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use thiserror::Error;
use toml_edit::{DocumentMut, Item, TableLike};
use uuid::Uuid;

use crate::{DirEntry, Entry, FileEntry};

pub struct Collection {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub(crate) local_env: Environment,
}

impl Collection {
    pub fn from_path(
        path: impl AsRef<Path>,
        local_env: Environment,
    ) -> Result<Self, CollectionLoadError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|source| CollectionLoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;

        if !metadata.is_dir() {
            return Err(CollectionLoadError::UnsupportedPath {
                path: path.to_path_buf(),
            });
        }

        let entries = load_directory(path, Some(&local_env.path))?.entries;

        Ok(Self {
            path: path.to_path_buf(),
            entries,
            local_env,
        })
    }

    pub fn save_files(&mut self) -> Result<(), CollectionSaveError> {
        fs::create_dir_all(&self.path).map_err(|source| CollectionSaveError::Write {
            path: self.path.clone(),
            source,
        })?;

        for entry in &mut self.entries {
            save_entry(entry)?;
        }

        Ok(())
    }

    pub fn local_env(&self) -> &Environment {
        &self.local_env
    }
}

fn load_directory(
    path: &Path,
    excluded_path: Option<&Path>,
) -> Result<DirEntry, CollectionLoadError> {
    let directory = fs::read_dir(path).map_err(|source| CollectionLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut paths = directory
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| CollectionLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();

    let mut entries = Vec::new();
    for child_path in paths {
        if excluded_path == Some(child_path.as_path()) {
            continue;
        }

        let file_type = fs::symlink_metadata(&child_path)
            .map_err(|source| CollectionLoadError::Read {
                path: child_path.clone(),
                source,
            })?
            .file_type();

        if file_type.is_dir() {
            entries.push(Entry::Directory(load_directory(
                &child_path,
                excluded_path,
            )?));
        } else if file_type.is_file()
            && child_path
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("toml")
        {
            entries.push(Entry::File(load_file(&child_path)?));
        }
    }

    crate::order::apply(path, &mut entries).map_err(|source| CollectionLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(DirEntry {
        path: path.to_path_buf(),
        name: file_name(path),
        entries,
    })
}

pub(crate) fn load_file(path: &Path) -> Result<FileEntry, CollectionLoadError> {
    let raw_content = fs::read_to_string(path).map_err(|source| CollectionLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut entry: FileEntry =
        toml::from_str(&raw_content).map_err(|source| CollectionLoadError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    entry.path = path.to_path_buf();
    entry.raw_content = raw_content;

    Ok(entry)
}

fn save_entry(entry: &mut Entry) -> Result<(), CollectionSaveError> {
    match entry {
        Entry::File(file) => save_file(file),
        Entry::Directory(directory) => {
            fs::create_dir_all(&directory.path).map_err(|source| CollectionSaveError::Write {
                path: directory.path.clone(),
                source,
            })?;

            for entry in &mut directory.entries {
                save_entry(entry)?;
            }

            Ok(())
        }
    }
}

pub(crate) fn save_file(entry: &mut FileEntry) -> Result<(), CollectionSaveError> {
    if let Some(parent) = entry.path.parent() {
        fs::create_dir_all(parent).map_err(|source| CollectionSaveError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let rendered =
        toml::to_string_pretty(entry).map_err(|source| CollectionSaveError::Serialize {
            path: entry.path.clone(),
            source,
        })?;
    let updates = rendered
        .parse::<DocumentMut>()
        .map_err(|source| CollectionSaveError::Edit {
            path: entry.path.clone(),
            source,
        })?;

    let mut document =
        entry
            .raw_content
            .parse::<DocumentMut>()
            .map_err(|source| CollectionSaveError::Edit {
                path: entry.path.clone(),
                source,
            })?;

    // Optional fields omitted by serialization must also disappear from the file.
    if let Some(request) = document
        .get_mut("request")
        .and_then(Item::as_table_like_mut)
    {
        for field in ["body", "query"] {
            if updates["request"].get(field).is_none() {
                request.remove(field);
            }
        }
    }

    merge_table(document.as_table_mut(), updates.as_table());

    let raw_content = document.to_string();
    write_file_atomically(&entry.path, raw_content.as_bytes()).map_err(|source| {
        CollectionSaveError::Write {
            path: entry.path.clone(),
            source,
        }
    })?;

    entry.raw_content = raw_content;

    Ok(())
}

fn merge_table(target: &mut dyn TableLike, updates: &dyn TableLike) {
    for (key, update) in updates.iter() {
        if let Some(current) = target.get_mut(key) {
            merge_item(current, update);
        } else {
            target.insert(key, update.clone());
        }
    }
}

fn merge_item(target: &mut Item, update: &Item) {
    if let (Some(target), Some(update)) = (target.as_table_like_mut(), update.as_table_like()) {
        merge_table(target, update);

        return;
    }

    match (target, update) {
        (Item::Value(target), Item::Value(update)) => {
            let decor = target.decor().clone();
            *target = update.clone();
            *target.decor_mut() = decor;
        }
        (target, update) => *target = update.clone(),
    }
}

fn write_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let permissions = match fs::metadata(path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "The request file is read-only.",
                ));
            }

            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let temporary = path.with_file_name(format!(".request-eagle-{}.tmp", Uuid::new_v4()));
    let mut file = fs::File::create_new(&temporary)?;
    let result = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }

        file.write_all(content)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temporary, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }

    result
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[derive(Debug, Error)]
pub enum CollectionLoadError {
    #[error("failed to read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    #[error("failed to parse {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("unsupported collection path {}", .path.display())]
    UnsupportedPath { path: PathBuf },
}

#[derive(Debug, Error)]
pub enum CollectionSaveError {
    #[error("failed to serialize collection file {}: {source}", .path.display())]
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },

    #[error("failed to edit collection file {}: {source}", .path.display())]
    Edit {
        path: PathBuf,
        source: toml_edit::TomlError,
    },

    #[error("failed to write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },
}
