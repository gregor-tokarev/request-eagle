use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use uuid::Uuid;

use super::catalog::ENVIRONMENT_FILE_NAME;
use crate::collection::is_reserved;
use crate::{Collection, CollectionEditError, CollectionRegistry, CollectionSaveError, FileEntry};
use request::{Request, RequestScripts};

/// A collection read from another application's export, ready to be written
/// as a new collection.
pub struct ImportedCollection {
    pub name: String,
    pub variables: HashMap<String, String>,
    pub scripts: RequestScripts,
    pub items: Vec<ImportedItem>,
}

pub enum ImportedItem {
    Folder {
        name: String,
        items: Vec<ImportedItem>,
    },
    Request {
        name: String,
        request: Request,
    },
}

/// Keeps generated file names, with a number and extension, well within the
/// 255-byte limit of common filesystems.
const MAX_FILE_STEM_BYTES: usize = 120;

impl CollectionRegistry {
    /// Writes an imported collection beside the others, numbering its name
    /// when it is taken. A failed import removes everything it wrote.
    pub fn import_collection(
        &mut self,
        imported: ImportedCollection,
    ) -> Result<PathBuf, CollectionEditError> {
        let directory = self
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("No collections directory is configured."))?;
        fs::create_dir_all(directory)?;
        let path = create_unique(
            directory,
            &file_stem(&imported.name, "Imported Collection"),
            "",
            &[],
            |path| fs::create_dir(path),
        )?;

        match write_collection(&path, imported) {
            Ok(collection) => {
                self.collections.push(collection);
                Ok(path)
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&path);
                Err(error)
            }
        }
    }
}

fn write_collection(
    path: &Path,
    imported: ImportedCollection,
) -> Result<Collection, CollectionEditError> {
    let mut collection = Collection {
        path: path.to_path_buf(),
        entries: Vec::new(),
        local_env: Environment {
            path: path.join(ENVIRONMENT_FILE_NAME),
            entries: HashMap::new(),
        },
        scripts: RequestScripts::default(),
    };
    collection.save_settings(imported.variables, imported.scripts)?;

    write_items(path, &imported.items, &collection.reserved_paths())?;

    // Loading what was written keeps the registry identical to a restart.
    Ok(Collection::from_path(path, collection.local_env)?)
}

fn write_items(
    parent: &Path,
    items: &[ImportedItem],
    reserved: &[PathBuf],
) -> Result<(), CollectionEditError> {
    let mut paths = Vec::with_capacity(items.len());

    for item in items {
        let path = match item {
            ImportedItem::Folder { name, items } => {
                let path =
                    create_unique(parent, &file_stem(name, "Folder"), "", reserved, |path| {
                        fs::create_dir(path)
                    })?;
                write_items(&path, items, reserved)?;

                path
            }
            ImportedItem::Request { name, request } => {
                let entry = FileEntry {
                    raw_content: String::new(),
                    path: PathBuf::new(),
                    id: Uuid::new_v4().to_string(),
                    name: name.clone(),
                    schema_version: 1,
                    request: request.clone(),
                };
                let content = toml::to_string_pretty(&entry).map_err(|source| {
                    CollectionSaveError::Serialize {
                        path: parent.join(name),
                        source,
                    }
                })?;

                create_unique(
                    parent,
                    &file_stem(name, "Request"),
                    ".toml",
                    reserved,
                    |path| fs::File::create_new(path)?.write_all(content.as_bytes()),
                )?
            }
        };

        paths.push(path);
    }

    // Folders and requests keep the order of the source.
    crate::order::save(parent, &paths)?;

    Ok(())
}

/// Runs `create` for the first of `stem`, `stem 2`, … that is neither
/// reserved nor already taken, and returns that path.
fn create_unique(
    parent: &Path,
    stem: &str,
    extension: &str,
    reserved: &[PathBuf],
    mut create: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    for number in 1.. {
        let name = if number == 1 {
            stem.to_owned()
        } else {
            format!("{stem} {number}")
        };
        let path = parent.join(format!("{name}{extension}"));
        if is_reserved(reserved, &path) {
            continue;
        }

        match create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    unreachable!()
}

/// A file name for `name` without path separators, control characters or
/// leading dots, which would hide the file or name the collection's own files.
fn file_stem(name: &str, fallback: &str) -> String {
    let mut stem = String::new();
    for character in name.chars() {
        let character = if matches!(character, '/' | '\\' | ':') || character.is_control() {
            '-'
        } else {
            character
        };
        if stem.len() + character.len_utf8() > MAX_FILE_STEM_BYTES {
            break;
        }
        stem.push(character);
    }

    let stem = stem.trim().trim_start_matches('.').trim();
    if stem.is_empty() {
        fallback.to_owned()
    } else {
        stem.to_owned()
    }
}
