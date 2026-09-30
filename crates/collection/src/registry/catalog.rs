use std::{
    fs, io,
    path::{Path, PathBuf},
};

use environment::{Environment, EnvironmentLoadError};
use thiserror::Error;

use crate::{Collection, CollectionLoadError, Entry, FileEntry};

pub(super) const ENVIRONMENT_FILE_NAME: &str = "environment.toml";

#[derive(Default)]
pub struct CollectionRegistry {
    pub(super) collections: Vec<Collection>,
    pub(super) directory: Option<PathBuf>,
}

impl CollectionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, CollectionRegistryLoadError> {
        let path = path.as_ref();
        let mut registry = Self {
            directory: Some(path.to_path_buf()),
            ..Self::new()
        };
        let directory = match fs::read_dir(path) {
            Ok(directory) => directory,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(registry),
            Err(source) => {
                return Err(CollectionRegistryLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        let mut collection_paths = directory
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|source| {
                    CollectionRegistryLoadError::Read {
                        path: path.to_path_buf(),
                        source,
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        collection_paths.sort();

        for collection_path in collection_paths {
            let metadata = fs::metadata(&collection_path).map_err(|source| {
                CollectionRegistryLoadError::Read {
                    path: collection_path.clone(),
                    source,
                }
            })?;

            if !metadata.is_dir() {
                continue;
            }

            let environment_path = collection_path.join(ENVIRONMENT_FILE_NAME);
            let environment = Environment::from_file(&environment_path).map_err(|source| {
                CollectionRegistryLoadError::Environment {
                    path: environment_path,
                    source: Box::new(source),
                }
            })?;

            let collection =
                Collection::from_path(&collection_path, environment).map_err(|source| {
                    CollectionRegistryLoadError::Collection {
                        path: collection_path,
                        source: Box::new(source),
                    }
                })?;
            registry.collections.push(collection);
        }

        Ok(registry)
    }

    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    pub fn file(&self, path: &Path) -> Option<&FileEntry> {
        self.collections.iter().find_map(|collection| {
            path.starts_with(&collection.path)
                .then(|| find_file(&collection.entries, path))
                .flatten()
        })
    }
}

fn find_file<'a>(entries: &'a [Entry], path: &Path) -> Option<&'a FileEntry> {
    entries.iter().find_map(|entry| match entry {
        Entry::File(file) if file.path == path => Some(file),
        Entry::Directory(folder) if path.starts_with(&folder.path) => {
            find_file(&folder.entries, path)
        }
        _ => None,
    })
}

#[derive(Debug, Error)]
pub enum CollectionRegistryLoadError {
    #[error("failed to read collections from {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    #[error("failed to load the environment for {}: {source}", .path.display())]
    Environment {
        path: PathBuf,
        source: Box<EnvironmentLoadError>,
    },

    #[error("failed to load collection {}: {source}", .path.display())]
    Collection {
        path: PathBuf,
        source: Box<CollectionLoadError>,
    },
}
