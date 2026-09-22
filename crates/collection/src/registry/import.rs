use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use environment::Environment;
use uuid::Uuid;

use super::mutations::rebase_entries;
use crate::{
    Collection, CollectionEditError, CollectionRegistry, CollectionSaveError, FileEntry,
    HttpRequest, Request,
};

const STAGING_PREFIX: &str = ".request-eagle-import-";

pub(super) fn is_staging_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(STAGING_PREFIX))
        .is_some_and(|suffix| Uuid::parse_str(suffix).is_ok())
}

#[derive(Clone, Debug)]
pub struct ImportedRequest {
    pub name: String,
    pub folders: Vec<String>,
    pub request: HttpRequest,
}

#[derive(Clone, Debug)]
pub struct ImportedFile {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub request: HttpRequest,
    pub collection_path: PathBuf,
}

impl CollectionRegistry {
    /// Saves a complete import before publishing it in the collection directory.
    pub fn import_requests(
        &mut self,
        name: &str,
        requests: Vec<ImportedRequest>,
    ) -> Result<Vec<ImportedFile>, CollectionEditError> {
        if requests.is_empty() {
            return Err(io::Error::other("The import contains no requests.").into());
        }

        let directory = self
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("No collections directory is configured."))?;
        create_private_directory(directory, true)?;
        // The registry skips this reserved name until the complete import is renamed.
        let staging = directory.join(format!("{STAGING_PREFIX}{}", Uuid::new_v4()));
        create_private_directory(&staging, false)?;

        let result = stage_requests(&staging, requests).and_then(|(mut collection, mut files)| {
            let base = safe_name(name, "Imported Collection");

            for number in 1.. {
                let destination = numbered_path(directory, &base, number, false);

                match publish_directory(&staging, &destination) {
                    Ok(()) => {
                        rebase_entries(&mut collection.entries, &staging, &destination);
                        collection.path = destination.clone();
                        collection.local_env.path = destination.join("environment.toml");

                        for file in &mut files {
                            file.path = destination.join(file.path.strip_prefix(&staging).unwrap());
                            file.collection_path = destination.clone();
                        }

                        self.collections.push(collection);
                        return Ok(files);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error.into()),
                }
            }

            unreachable!()
        });

        if result.is_err()
            && let Err(cleanup) = fs::remove_dir_all(&staging)
        {
            return Err(io::Error::other(format!(
                "{}; could not remove staged import {}: {cleanup}",
                result.unwrap_err(),
                staging.display()
            ))
            .into());
        }

        result
    }
}

fn stage_requests(
    staging: &Path,
    requests: Vec<ImportedRequest>,
) -> Result<(Collection, Vec<ImportedFile>), CollectionEditError> {
    let mut folders = HashMap::<Vec<String>, PathBuf>::new();
    let mut order = HashMap::<PathBuf, Vec<PathBuf>>::new();
    let mut files = Vec::with_capacity(requests.len());

    for imported in requests {
        let mut parent = staging.to_path_buf();
        let mut folder_key = Vec::new();

        for folder in imported.folders {
            folder_key.push(folder.clone());

            if let Some(path) = folders.get(&folder_key) {
                parent = path.clone();
                continue;
            }

            let path = unused_path(&parent, &safe_name(&folder, "Folder"), false)?;
            create_private_directory(&path, false)?;
            order.entry(parent).or_default().push(path.clone());
            folders.insert(folder_key.clone(), path.clone());
            parent = path;
        }

        let name = if imported.name.trim().is_empty() {
            "Imported Request".to_owned()
        } else {
            imported.name
        };
        let path = unused_path(&parent, &safe_name(&name, "Request"), true)?;
        let entry = FileEntry {
            path: path.clone(),
            raw_content: String::new(),
            id: Uuid::new_v4().to_string(),
            name: name.clone(),
            schema_version: 1,
            request: Request::Http(imported.request.clone()),
        };
        let content =
            toml::to_string_pretty(&entry).map_err(|source| CollectionSaveError::Serialize {
                path: path.clone(),
                source,
            })?;
        write_private_file(&path, content.as_bytes())?;
        order.entry(parent).or_default().push(path.clone());
        files.push(ImportedFile {
            id: entry.id,
            path,
            name,
            request: imported.request,
            collection_path: staging.to_path_buf(),
        });
    }

    for (parent, paths) in order {
        let names = paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy())
            .collect::<Vec<_>>();
        let content = serde_json::to_vec_pretty(&names).map_err(io::Error::other)?;
        write_private_file(&parent.join(".request-eagle-order.json"), &content)?;
    }

    let collection = Collection::from_path(
        staging,
        Environment {
            path: staging.join("environment.toml"),
            entries: HashMap::new(),
        },
    )?;

    Ok((collection, files))
}

fn safe_name(name: &str, fallback: &str) -> String {
    let mut safe = String::new();

    for character in name.trim().chars() {
        let character = if character.is_control() || "/\\:<>\"|?*".contains(character) {
            '_'
        } else {
            character
        };

        if safe.len() + character.len_utf8() > 180 {
            break;
        }

        safe.push(character);
    }

    let safe = safe.trim_matches(['.', ' ']);

    if safe.is_empty() {
        return fallback.to_owned();
    }

    let stem = safe.split('.').next().unwrap().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']))
        || stem
            .strip_prefix("LPT")
            .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']));

    if reserved {
        format!("_{safe}")
    } else {
        safe.to_owned()
    }
}

fn numbered_path(parent: &Path, base: &str, number: usize, file: bool) -> PathBuf {
    let suffix = if file { ".toml" } else { "" };

    parent.join(if number == 1 {
        format!("{base}{suffix}")
    } else {
        format!("{base} {number}{suffix}")
    })
}

fn unused_path(parent: &Path, base: &str, file: bool) -> io::Result<PathBuf> {
    for number in 1.. {
        let path = numbered_path(parent, base, number, file);

        // The collection loader reserves this filename for local variables.
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case("environment.toml")
        {
            continue;
        }

        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }

    unreachable!()
}

fn create_private_directory(path: &Path, recursive: bool) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);

    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }

    builder.create(path)
}

fn write_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(unix)]
fn publish_directory(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(not(unix))]
fn publish_directory(source: &Path, destination: &Path) -> io::Result<()> {
    // Windows does not replace an existing directory when renaming a directory.
    fs::rename(source, destination)
}
