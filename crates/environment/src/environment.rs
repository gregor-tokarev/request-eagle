use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

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

        fs::write(path, source).map_err(|source| EnvironmentSaveError::Write {
            path: path.to_path_buf(),
            source,
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
