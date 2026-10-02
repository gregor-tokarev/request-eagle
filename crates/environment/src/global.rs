use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{Environment, EnvironmentSaveError};

const EXTENSION: &str = "toml";

/// Leaves room in the 255-byte file name limit of common filesystems for a
/// number and the extension.
const MAX_IMPORTED_NAME_BYTES: usize = 120;

/// Named environments that apply to requests in every collection. Each one is
/// a TOML file in a single directory, and its file name is the environment name.
#[derive(Clone, Debug)]
pub struct GlobalEnvironments {
    directory: PathBuf,
}

impl GlobalEnvironments {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.{EXTENSION}"))
    }

    /// Environment names in case-insensitive order. A missing directory has none.
    pub fn names(&self) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let mut names = Vec::new();
        for entry in entries {
            let path = entry?.path();

            if path.extension().and_then(|extension| extension.to_str()) != Some(EXTENSION)
                || !path.is_file()
            {
                continue;
            }

            if let Some(name) = path.file_stem().and_then(|name| name.to_str())
                && valid_name(name)
            {
                names.push(name.to_owned());
            }
        }

        names.sort_by_key(|name| name.to_lowercase());
        Ok(names)
    }

    /// Create an empty environment named `base`, or `base 2`, `base 3`, and so on.
    pub fn create(&self, base: &str) -> Result<String, GlobalEnvironmentError> {
        if !valid_name(base) {
            return Err(GlobalEnvironmentError::InvalidName);
        }

        fs::create_dir_all(&self.directory)?;

        for number in 1.. {
            let name = if number == 1 {
                base.to_owned()
            } else {
                format!("{base} {number}")
            };

            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path(&name))
            {
                Ok(_) => return Ok(name),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }

        unreachable!()
    }

    /// Saves variables from another app as a new environment named after
    /// `base`, or `base 2`, `base 3`, and so on. Path separators in the name
    /// become `-`, and a long name is shortened, so any name fits.
    pub fn import(
        &self,
        base: &str,
        entries: HashMap<String, String>,
    ) -> Result<String, GlobalEnvironmentError> {
        let mut name = String::new();
        for character in base.chars() {
            let character = match character {
                '/' | '\\' | ':' => '-',
                character if character.is_control() => ' ',
                character => character,
            };
            if name.len() + character.len_utf8() > MAX_IMPORTED_NAME_BYTES {
                break;
            }
            name.push(character);
        }
        let base = name.trim().trim_start_matches('.').trim();
        let name = self.create(if base.is_empty() { "Imported" } else { base })?;

        let environment = Environment {
            path: self.path(&name),
            entries,
        };
        if let Err(error) = environment.save_file() {
            let _ = fs::remove_file(&environment.path);
            return Err(error.into());
        }

        Ok(name)
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<String, GlobalEnvironmentError> {
        let to = to.trim();
        if !valid_name(to) {
            return Err(GlobalEnvironmentError::InvalidName);
        }

        let source = self.path(from);
        let destination = self.path(to);
        if source == destination {
            return Ok(to.to_owned());
        }

        // Allow case-only renames on case-insensitive filesystems, but never
        // replace a different environment.
        if fs::symlink_metadata(&destination).is_ok() && !same_file(&source, &destination)? {
            return Err(GlobalEnvironmentError::AlreadyExists);
        }

        fs::rename(source, destination)?;
        Ok(to.to_owned())
    }

    pub fn delete(&self, name: &str) -> Result<(), GlobalEnvironmentError> {
        match fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name == name.trim()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

#[cfg(unix)]
fn same_file(first: &Path, second: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let first = fs::symlink_metadata(first)?;
    let second = fs::symlink_metadata(second)?;

    Ok(first.dev() == second.dev() && first.ino() == second.ino())
}

#[cfg(not(unix))]
fn same_file(first: &Path, second: &Path) -> io::Result<bool> {
    Ok(first.to_string_lossy().to_lowercase() == second.to_string_lossy().to_lowercase())
}

#[derive(Debug, Error)]
pub enum GlobalEnvironmentError {
    #[error("Enter a name without path separators, a leading dot, or control characters.")]
    InvalidName,
    #[error("An environment with that name already exists.")]
    AlreadyExists,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Save(#[from] EnvironmentSaveError),
}
