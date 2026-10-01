use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use uuid::Uuid;

use super::catalog::ENVIRONMENT_FILE_NAME;
use crate::collection::{is_reserved, render};
use crate::{Collection, CollectionEditError, CollectionRegistry, FileEntry};
use request::{Request, RequestScripts};

/// A collection read from another application's export, ready to be written
/// as a new collection.
pub struct ImportedCollection {
    pub name: String,
    pub variables: HashMap<String, String>,
    pub scripts: RequestScripts,
    pub items: Vec<ImportedItem>,
}

// Most items are requests, so boxing them would not make imports smaller.
#[allow(clippy::large_enum_variant)]
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

impl ImportedCollection {
    /// Writes the collection as a new directory in `directory`, numbering its
    /// name when it is taken. A failed write removes everything it wrote.
    ///
    /// Large imports take a while, so this works without the registry and can
    /// run in the background; add the result with
    /// [`CollectionRegistry::add_collection`].
    pub fn write(self, directory: &Path) -> Result<Collection, CollectionEditError> {
        fs::create_dir_all(directory)?;
        let path = create_unique(
            directory,
            &file_stem(&self.name, "Imported Collection"),
            "",
            1,
            &[],
            |path| fs::create_dir(path),
        )?
        .0;

        write_collection(&path, self).inspect_err(|_| {
            let _ = fs::remove_dir_all(&path);
        })
    }
}

impl CollectionRegistry {
    /// Adds a collection written outside the registry, such as an import.
    pub fn add_collection(&mut self, collection: Collection) {
        self.collections.push(collection);
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

    // Loading what was written keeps the registry identical to a restart,
    // and all of it must load.
    let mut skipped = Vec::new();
    let collection = Collection::from_path(path, collection.local_env, &mut skipped)?;
    if let Some(skipped) = skipped.first() {
        return Err(
            io::Error::other(format!("{}: {}", skipped.path.display(), skipped.error)).into(),
        );
    }

    Ok(collection)
}

fn write_items(
    parent: &Path,
    items: &[ImportedItem],
    reserved: &[PathBuf],
) -> Result<(), CollectionEditError> {
    let mut paths = Vec::with_capacity(items.len());
    // The next number to try for each name, so that many items with the same
    // name do not retry every number taken before them.
    let mut next_numbers: HashMap<String, usize> = HashMap::new();

    for item in items {
        let (stem, extension) = match item {
            ImportedItem::Folder { name, .. } => (file_stem(name, "Folder"), ""),
            ImportedItem::Request { name, .. } => (file_stem(name, "Request"), ".toml"),
        };
        // Filesystems can ignore case, so names that differ only in case share numbers.
        let next_number = next_numbers
            .entry(format!("{stem}{extension}").to_lowercase())
            .or_insert(1);

        let (path, number) = match item {
            ImportedItem::Folder { items, .. } => {
                let created = create_unique(parent, &stem, "", *next_number, reserved, |path| {
                    fs::create_dir(path)
                })?;
                write_items(&created.0, items, reserved)?;

                created
            }
            ImportedItem::Request { name, request } => {
                let entry = FileEntry {
                    raw_content: String::new(),
                    path: parent.join(name),
                    id: Uuid::new_v4().to_string(),
                    name: name.clone(),
                    schema_version: 1,
                    request: request.clone(),
                };
                let content = render(&entry)?.to_string();

                create_unique(parent, &stem, ".toml", *next_number, reserved, |path| {
                    fs::File::create_new(path)?.write_all(content.as_bytes())
                })?
            }
        };

        *next_number = number + 1;
        paths.push(path);
    }

    // Folders and requests keep the order of the source.
    crate::order::save(parent, &paths)?;

    Ok(())
}

/// Runs `create` for the first of `stem`, `stem 2`, … from `first_number`
/// that is neither reserved nor already taken, and returns that path and its
/// number.
fn create_unique(
    parent: &Path,
    stem: &str,
    extension: &str,
    first_number: usize,
    reserved: &[PathBuf],
    mut create: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<(PathBuf, usize)> {
    for number in first_number.. {
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
            Ok(()) => return Ok((path, number)),
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
