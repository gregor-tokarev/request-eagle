use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use uuid::Uuid;

use super::catalog::ENVIRONMENT_FILE_NAME;
use super::mutations::{check_directory_name, find_entry};
use crate::collection::{is_reserved, render};
use crate::{Collection, CollectionEditError, CollectionRegistry, DirEntry, Entry, FileEntry};
use request::{HttpRequest, Method, Request};

impl CollectionRegistry {
    pub fn create_collection(&mut self) -> Result<PathBuf, CollectionEditError> {
        let directory = self.collections_directory()?;
        let path = create_directory(&directory, "New Collection")?;
        self.add_empty_collection(&path);

        Ok(path)
    }

    /// Creates a collection with exactly this name. Fails when another
    /// collection already uses the name.
    pub fn create_named_collection(&mut self, name: &str) -> Result<PathBuf, CollectionEditError> {
        let name = name.trim();
        check_directory_name(name)?;
        if self.collection_name_taken(name) {
            return Err(CollectionEditError::AlreadyExists);
        }

        let path = self.collections_directory()?.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(CollectionEditError::AlreadyExists);
            }
            Err(error) => return Err(error.into()),
        }
        self.add_empty_collection(&path);

        Ok(path)
    }

    /// Whether a collection already has this name. Names that differ only
    /// in case count as the same, as they do on some file systems.
    pub fn collection_name_taken(&self, name: &str) -> bool {
        let name = name.trim().to_lowercase();
        self.collections.iter().any(|collection| {
            collection
                .path
                .file_name()
                .is_some_and(|existing| existing.to_string_lossy().to_lowercase() == name)
        })
    }

    /// The first of "New Collection", "New Collection 2" and so on that no
    /// collection uses and nothing in the collections directory occupies.
    pub fn free_collection_name(&self) -> String {
        let mut taken: HashSet<String> = self
            .collections
            .iter()
            .filter_map(|collection| collection.path.file_name())
            .map(|name| name.to_string_lossy().to_lowercase())
            .collect();
        if let Some(entries) = self
            .directory
            .as_ref()
            .and_then(|directory| fs::read_dir(directory).ok())
        {
            taken.extend(
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().to_lowercase()),
            );
        }

        (1..)
            .map(|number| match number {
                1 => "New Collection".to_owned(),
                number => format!("New Collection {number}"),
            })
            .find(|name| !taken.contains(&name.to_lowercase()))
            .expect("a free collection name")
    }

    fn collections_directory(&self) -> Result<PathBuf, CollectionEditError> {
        let directory = self
            .directory
            .clone()
            .ok_or_else(|| io::Error::other("No collections directory is configured."))?;
        fs::create_dir_all(&directory)?;

        Ok(directory)
    }

    fn add_empty_collection(&mut self, path: &Path) {
        self.collections.push(Collection {
            path: path.to_path_buf(),
            entries: Vec::new(),
            local_env: Environment {
                path: path.join(ENVIRONMENT_FILE_NAME),
                entries: HashMap::new(),
                secrets: Default::default(),
            },
            scripts: Default::default(),
            auth: Default::default(),
            outside_changes: 0,
        });
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
        self.create_request_with(
            parent,
            "New Request",
            Request::Http(HttpRequest {
                method: Method::Get,
                path: "/".to_owned(),
                headers: Vec::new(),
                body: None,
                query: Vec::new(),
                path_variables: Vec::new(),
                auth: Default::default(),
                scripts: Default::default(),
                settings: Default::default(),
            }),
        )
    }

    pub fn create_request_with(
        &mut self,
        parent: &Path,
        name: &str,
        request: Request,
    ) -> Result<PathBuf, CollectionEditError> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(CollectionEditError::InvalidName);
        }
        let base_name = name;
        let reserved = self.reserved_paths();
        let entries = self
            .entries_mut(parent)
            .ok_or(CollectionEditError::NotFound)?;
        let id = Uuid::new_v4().to_string();

        for number in 1.. {
            let name = if number == 1 {
                base_name.to_owned()
            } else {
                format!("{base_name} {number}")
            };
            let filename = name.replace(['/', '\\', ':'], "-");
            let path = parent.join(format!("{filename}.toml"));
            if is_reserved(&reserved, &path) {
                continue;
            }
            let mut entry = FileEntry {
                path: path.clone(),
                raw_content: String::new(),
                outside_changes: 0,
                id: id.clone(),
                name,
                schema_version: 1,
                request: request.clone(),
            };
            entry.raw_content = render(&entry)?.to_string();
            let mut file = match fs::File::create_new(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };

            if let Err(error) = file.write_all(entry.raw_content.as_bytes()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error.into());
            }

            drop(file);
            persist_created_order(parent, entries, &path, false)?;
            entries.push(Entry::File(entry));
            return Ok(path);
        }

        unreachable!()
    }

    /// Every collection's own files, which entries cannot replace.
    pub(super) fn reserved_paths(&self) -> Vec<PathBuf> {
        self.collections
            .iter()
            .flat_map(Collection::reserved_paths)
            .collect()
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
