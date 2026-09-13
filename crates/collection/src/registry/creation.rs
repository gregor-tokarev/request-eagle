use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use uuid::Uuid;

use super::mutations::find_entry;
use crate::{
    Collection, CollectionEditError, CollectionRegistry, CollectionSaveError, DirEntry, Entry,
    FileEntry, HttpRequest, Method, Request,
};

impl CollectionRegistry {
    pub fn create_collection(&mut self) -> Result<PathBuf, CollectionEditError> {
        let directory = self
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("No collections directory is configured."))?;
        fs::create_dir_all(directory)?;
        let path = create_directory(directory, "New Collection")?;

        self.collections.push(Collection {
            path: path.clone(),
            entries: Vec::new(),
            local_env: Environment {
                path: path.join("environment.toml"),
                entries: HashMap::new(),
            },
        });

        Ok(path)
    }

    pub fn create_folder(&mut self, parent: &Path) -> Result<PathBuf, CollectionEditError> {
        let entries = self
            .entries_mut(parent)
            .ok_or(CollectionEditError::NotFound)?;
        let path = create_directory(parent, "New Folder")?;
        persist_created_order(parent, entries, &path, true)?;
        entries.push(Entry::Directory(DirEntry {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: path.clone(),
            entries: Vec::new(),
        }));

        Ok(path)
    }

    pub fn create_request(&mut self, parent: &Path) -> Result<PathBuf, CollectionEditError> {
        let entries = self
            .entries_mut(parent)
            .ok_or(CollectionEditError::NotFound)?;
        let id = Uuid::new_v4().to_string();

        for number in 1.. {
            let name = if number == 1 {
                "New Request".to_owned()
            } else {
                format!("New Request {number}")
            };
            let path = parent.join(format!("{name}.toml"));
            let mut entry = FileEntry {
                path: path.clone(),
                raw_content: String::new(),
                id: id.clone(),
                name,
                schema_version: 1,
                request: Request::Http(HttpRequest {
                    method: Method::Get,
                    path: "/".to_owned(),
                    headers: Vec::new(),
                    body: None,
                    query: None,
                }),
            };
            let content = toml::to_string_pretty(&entry).map_err(|source| {
                CollectionSaveError::Serialize {
                    path: path.clone(),
                    source,
                }
            })?;
            let mut file = match fs::File::create_new(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };

            if let Err(error) = file.write_all(content.as_bytes()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error.into());
            }

            drop(file);
            persist_created_order(parent, entries, &path, false)?;
            entry.raw_content = content;
            entries.push(Entry::File(entry));
            return Ok(path);
        }

        unreachable!()
    }

    pub(super) fn entries_mut(&mut self, parent: &Path) -> Option<&mut Vec<Entry>> {
        for collection in &mut self.collections {
            if collection.path == parent {
                return Some(&mut collection.entries);
            }

            if let Some(Entry::Directory(folder)) = find_entry(&mut collection.entries, parent) {
                return Some(&mut folder.entries);
            }
        }

        None
    }
}

fn create_directory(parent: &Path, base: &str) -> io::Result<PathBuf> {
    for number in 1.. {
        let name = if number == 1 {
            base.to_owned()
        } else {
            format!("{base} {number}")
        };
        let path = parent.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    unreachable!()
}

fn persist_created_order(
    parent: &Path,
    entries: &[Entry],
    path: &Path,
    directory: bool,
) -> io::Result<()> {
    let paths: Vec<_> = entries
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .chain(std::iter::once(path.to_path_buf()))
        .collect();
    if let Err(error) = crate::order::save(parent, &paths) {
        if directory {
            let _ = fs::remove_dir(path);
        } else {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(())
}
