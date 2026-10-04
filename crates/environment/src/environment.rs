use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use thiserror::Error;
use uuid::Uuid;

pub struct Environment {
    pub path: PathBuf,
    pub entries: HashMap<String, String>,
}

impl Environment {
    /// Loads an environment file. A missing file loads as an empty environment.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, EnvironmentLoadError> {
        let path = path.as_ref();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(EnvironmentLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        Self::from_toml(path, &source)
    }

    pub fn save_file(&self) -> Result<(), EnvironmentSaveError> {
        let path = &self.path;
        let entries: BTreeMap<&str, &str> = self
            .entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();

        let source =
            toml::to_string_pretty(&entries).map_err(|source| EnvironmentSaveError::Serialize {
                path: path.to_path_buf(),
                source,
            })?;

        write_file_atomically(path, source.as_bytes()).map_err(|source| {
            EnvironmentSaveError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    pub fn resolve(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub(super) fn from_toml(path: &Path, source: &str) -> Result<Self, EnvironmentLoadError> {
        let entries: HashMap<String, String> =
            toml::from_str(source).map_err(|source| EnvironmentLoadError::Parse {
                path: path.to_path_buf(),
                source,
            })?;

        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }
}

/// Replaces the file whole, so a save that fails midway leaves the previous
/// variables rather than a part of the new ones.
fn write_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    // A link to variables shared from elsewhere stays a link: the file it
    // leads to is the one replaced.
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let path = target.as_path();

    let permissions = match fs::metadata(path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "The environment file is read-only.",
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

#[derive(Debug, Error)]
pub enum EnvironmentLoadError {
    #[error("failed to read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    #[error("failed to parse {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

#[derive(Debug, Error)]
pub enum EnvironmentSaveError {
    #[error("failed to serialize environment for {}: {source}", .path.display())]
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },

    #[error("failed to write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },
}
