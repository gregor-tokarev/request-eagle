use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_edit::{DocumentMut, Item};
use uuid::Uuid;

use crate::toml_merge::merge_table;
use crate::{CollectionEditError, DirEntry, Entry, FileEntry, FlowEntry};
use request::{Request, RequestScripts};

/// Collection-wide settings. Like `environment.toml`, loading skips it as a request.
const SETTINGS_FILE_NAME: &str = ".request-eagle-collection.toml";

pub struct Collection {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub(crate) local_env: Environment,
    pub(crate) scripts: RequestScripts,
}

#[derive(Default, Deserialize, Serialize)]
struct CollectionSettings {
    #[serde(default, skip_serializing_if = "RequestScripts::is_empty")]
    scripts: RequestScripts,
}

impl Collection {
    pub(crate) fn from_path(
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

        let excluded = [local_env.path.clone(), path.join(SETTINGS_FILE_NAME)];
        let entries = load_directory(path, &excluded)?.entries;
        let scripts = Self::load_scripts(path)?;

        Ok(Self {
            path: path.to_path_buf(),
            entries,
            local_env,
            scripts,
        })
    }

    /// Reads the scripts that run around every request in the collection at `path`.
    pub fn load_scripts(path: &Path) -> Result<RequestScripts, CollectionLoadError> {
        let path = path.join(SETTINGS_FILE_NAME);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RequestScripts::default());
            }
            Err(source) => return Err(CollectionLoadError::Read { path, source }),
        };

        toml::from_str::<CollectionSettings>(&source)
            .map(|settings| settings.scripts)
            .map_err(|source| CollectionLoadError::Parse { path, source })
    }

    /// Saves variables and scripts together. If the second file cannot be
    /// written, the first is restored, so a failed save changes nothing.
    pub(crate) fn save_settings(
        &mut self,
        variables: HashMap<String, String>,
        scripts: RequestScripts,
    ) -> Result<(), CollectionEditError> {
        let scripts_path = self.path.join(SETTINGS_FILE_NAME);
        let scripts_changed = self.scripts != scripts;
        let variables_changed = self.local_env.entries != variables;
        let previous_scripts = if scripts_changed && variables_changed {
            match fs::read(&scripts_path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            }
        } else {
            None
        };

        if scripts_changed {
            write_scripts(&scripts_path, &scripts)?;
        }

        if variables_changed {
            let environment = Environment {
                path: self.local_env.path.clone(),
                entries: variables,
            };

            if let Err(error) = environment.save_file() {
                if scripts_changed
                    && let Err(restore) = restore_file(&scripts_path, previous_scripts)
                {
                    return Err(io::Error::other(format!(
                        "{error}; could not restore the collection scripts: {restore}"
                    ))
                    .into());
                }

                return Err(error.into());
            }

            self.local_env = environment;
        }

        if scripts_changed {
            self.scripts = scripts;
        }

        Ok(())
    }

    /// The collection's own files, which requests and folders cannot replace.
    pub(crate) fn reserved_paths(&self) -> [PathBuf; 2] {
        [
            self.local_env.path.clone(),
            self.path.join(SETTINGS_FILE_NAME),
        ]
    }

    pub fn local_env(&self) -> &Environment {
        &self.local_env
    }

    pub fn scripts(&self) -> &RequestScripts {
        &self.scripts
    }
}

fn write_scripts(path: &Path, scripts: &RequestScripts) -> Result<(), CollectionSaveError> {
    if scripts.is_empty() {
        return restore_file(path, None).map_err(|source| CollectionSaveError::Write {
            path: path.to_path_buf(),
            source,
        });
    }

    let settings = CollectionSettings {
        scripts: scripts.clone(),
    };
    let content =
        toml::to_string_pretty(&settings).map_err(|source| CollectionSaveError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;

    write_file_atomically(path, content.as_bytes()).map_err(|source| CollectionSaveError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes `content`, or removes the file when there is none.
fn restore_file(path: &Path, content: Option<Vec<u8>>) -> io::Result<()> {
    match content {
        Some(content) => write_file_atomically(path, &content),
        None => match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

/// Whether `path` names one of the `reserved` files. Filesystems can ignore
/// case, so the comparison does too.
pub(crate) fn is_reserved(reserved: &[PathBuf], path: &Path) -> bool {
    reserved.iter().any(|reserved| {
        reserved.parent() == path.parent()
            && reserved
                .file_name()
                .zip(path.file_name())
                .is_some_and(|(a, b)| {
                    a.to_string_lossy()
                        .eq_ignore_ascii_case(&b.to_string_lossy())
                })
    })
}

fn load_directory(path: &Path, excluded: &[PathBuf]) -> Result<DirEntry, CollectionLoadError> {
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
        if excluded.contains(&child_path) {
            continue;
        }

        let file_type = fs::symlink_metadata(&child_path)
            .map_err(|source| CollectionLoadError::Read {
                path: child_path.clone(),
                source,
            })?
            .file_type();

        if file_type.is_dir() {
            entries.push(Entry::Directory(load_directory(&child_path, excluded)?));
        } else if file_type.is_file()
            && child_path
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("toml")
        {
            entries.push(load_item(&child_path)?);
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

/// A request or a flow, told apart by the `request` or `flow` table of its file.
pub(crate) fn load_item(path: &Path) -> Result<Entry, CollectionLoadError> {
    #[derive(Deserialize)]
    struct Item {
        id: String,
        name: String,
        schema_version: u8,
        request: Option<Request>,
        flow: Option<flow::Flow>,
    }

    let raw_content = fs::read_to_string(path).map_err(|source| CollectionLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let item: Item = toml::from_str(&raw_content).map_err(|source| CollectionLoadError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    match (item.request, item.flow) {
        (Some(request), _) => Ok(Entry::File(FileEntry {
            raw_content,
            path: path.to_path_buf(),
            id: item.id,
            name: item.name,
            schema_version: item.schema_version,
            request,
        })),
        (None, Some(flow)) => Ok(Entry::Flow(FlowEntry {
            path: path.to_path_buf(),
            id: item.id,
            name: item.name,
            schema_version: item.schema_version,
            flow,
        })),
        (None, None) => Err(CollectionLoadError::Parse {
            path: path.to_path_buf(),
            source: <toml::de::Error as serde::de::Error>::missing_field("request"),
        }),
    }
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

    // A request that changes protocol keeps none of its previous fields.
    if document
        .get("request")
        .and_then(|request| request.get("type"))
        .and_then(Item::as_str)
        != updates["request"].get("type").and_then(Item::as_str)
    {
        document.remove("request");
    }

    // Optional fields omitted by serialization must also disappear from the file.
    if let Some(request) = document
        .get_mut("request")
        .and_then(Item::as_table_like_mut)
    {
        for field in [
            "headers",
            "body",
            "query",
            "path_variables",
            "scripts",
            "tls",
            "method",
            "message",
            "metadata",
            "definition",
            "settings",
        ] {
            if updates["request"].get(field).is_none() {
                request.remove(field);
            }
        }

        // A body of another type keeps none of the previous type's fields.
        if let (Some(current), Some(update)) = (
            request.get_mut("body").and_then(Item::as_table_like_mut),
            updates["request"].get("body").and_then(Item::as_table_like),
        ) {
            for field in ["language", "text", "fields", "parts", "file"] {
                if update.get(field).is_none() {
                    current.remove(field);
                }
            }
        }

        // gRPC definitions, settings and scripts also omit optional fields,
        // such as import paths once they are removed.
        for table in ["definition", "settings", "scripts"] {
            if let (Some(current), Some(update)) = (
                request.get_mut(table).and_then(Item::as_table_like_mut),
                updates["request"].get(table).and_then(Item::as_table_like),
            ) {
                let stale = current
                    .iter()
                    .map(|(key, _)| key.to_owned())
                    .filter(|key| update.get(key).is_none())
                    .collect::<Vec<_>>();

                for key in stale {
                    current.remove(&key);
                }
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

/// Flows are written by the app and the CLI, so their file is written whole.
pub(crate) fn save_flow(entry: &FlowEntry) -> Result<(), CollectionSaveError> {
    let content =
        toml::to_string_pretty(entry).map_err(|source| CollectionSaveError::Serialize {
            path: entry.path.clone(),
            source,
        })?;

    write_file_atomically(&entry.path, content.as_bytes()).map_err(|source| {
        CollectionSaveError::Write {
            path: entry.path.clone(),
            source,
        }
    })
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
