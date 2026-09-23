use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use thiserror::Error;
use toml_edit::{Array, DocumentMut, Item, TableLike, Value};
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
        (Item::Value(target), Item::Value(update)) => merge_value(target, update),
        (target, update) => *target = update.clone(),
    }
}

fn merge_value(target: &mut Value, update: &Value) {
    match (target, update) {
        (Value::String(target), Value::String(update)) if target.value() == update.value() => {}
        (Value::Integer(target), Value::Integer(update)) if target.value() == update.value() => {}
        (Value::Float(target), Value::Float(update)) if target.value() == update.value() => {}
        (Value::Boolean(target), Value::Boolean(update)) if target.value() == update.value() => {}
        (Value::Datetime(target), Value::Datetime(update)) if target.value() == update.value() => {}
        (Value::Array(target), Value::Array(update)) => {
            if target
                .iter()
                .chain(update.iter())
                .all(|value| string_pair(value).is_some())
            {
                merge_key_value_rows(target, update);

                return;
            }

            while target.len() > update.len() {
                target.remove(target.len() - 1);
            }

            for (index, value) in update.iter().enumerate() {
                if let Some(current) = target.get_mut(index) {
                    merge_value(current, value);
                } else {
                    target.push_formatted(value.clone());
                }
            }
        }
        (Value::InlineTable(target), Value::InlineTable(update)) => merge_table(target, update),
        (target, update) => {
            let decor = target.decor().clone();
            *target = update.clone();
            *target.decor_mut() = decor;
        }
    }
}

fn string_pair(value: &Value) -> Option<(&str, &str)> {
    let values = value.as_array()?;
    if values.len() != 2 {
        return None;
    }

    Some((values.get(0)?.as_str()?, values.get(1)?.as_str()?))
}

fn merge_key_value_rows(target: &mut Array, update: &Array) {
    let mut rows: Vec<_> = target
        .iter()
        .cloned()
        .map(|value| Some((value, String::new())))
        .collect();

    // TOML attaches comments after a comma to the next value or the array tail.
    // Associate same-line comments with their preceding row before moving rows.
    for index in 1..rows.len() {
        let (value, _) = rows[index].as_mut().unwrap();
        let prefix = value
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .unwrap_or_default()
            .to_owned();
        let (comment, prefix) = split_row_comment(&prefix);
        value.decor_mut().set_prefix(prefix);
        rows[index - 1].as_mut().unwrap().1 = comment.to_owned();
    }

    let trailing = target.trailing().as_str().unwrap_or_default().to_owned();
    let trailing = if let Some(Some((_, comment))) = rows.last_mut() {
        let (row_comment, trailing) = split_row_comment(&trailing);
        *comment = row_comment.to_owned();
        trailing
    } else {
        &trailing
    };

    // Reserve exact matches first so duplicate keys retain their own annotations.
    let mut matched: Vec<_> = update
        .iter()
        .map(|value| {
            let index = rows.iter().position(|row| {
                row.as_ref()
                    .is_some_and(|(current, _)| string_pair(current) == string_pair(value))
            })?;

            rows[index].take()
        })
        .collect();

    target.clear();
    let mut preceding_comment = String::new();

    for (value, matched) in update.iter().zip(&mut matched) {
        if matched.is_none() {
            let key = string_pair(value).unwrap().0;
            if let Some(index) = rows.iter().position(|row| {
                row.as_ref()
                    .is_some_and(|(current, _)| string_pair(current).unwrap().0 == key)
            }) {
                *matched = rows[index].take();
            }
        }

        let (mut current, following_comment) = matched
            .take()
            .unwrap_or_else(|| (value.clone(), String::new()));
        merge_value(&mut current, value);
        let prefix = current
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .unwrap_or_default();
        let prefix = join_row_comment(&preceding_comment, prefix);
        current.decor_mut().set_prefix(prefix);
        target.push_formatted(current);
        preceding_comment = following_comment;
    }

    target.set_trailing(join_row_comment(&preceding_comment, trailing));
}

fn split_row_comment(text: &str) -> (&str, &str) {
    let end = text.find('\n').unwrap_or(text.len());
    if text[..end].contains('#') {
        text.split_at(end)
    } else {
        ("", text)
    }
}

fn join_row_comment(comment: &str, following: &str) -> String {
    if !comment.is_empty() && !following.starts_with(['\r', '\n']) {
        format!("{comment}\n{following}")
    } else {
        format!("{comment}{following}")
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
