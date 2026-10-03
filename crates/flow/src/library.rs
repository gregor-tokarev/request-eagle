use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::Flow;

const EXTENSION: &str = "toml";

/// A flow and the file it is saved in.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SavedFlow {
    #[serde(skip)]
    pub path: PathBuf,

    pub id: String,
    pub name: String,
    pub schema_version: u8,

    pub flow: Flow,
}

/// A file in the flows directory that could not be read.
#[derive(Clone, Debug)]
pub struct SkippedFlow {
    pub path: PathBuf,
    pub error: String,
}

/// The saved flows, apart from the collections whose requests they send.
/// Each one is a TOML file in a single directory, named after the flow when
/// it was created. Renaming a flow keeps its file.
pub struct FlowLibrary {
    directory: PathBuf,
    /// In case-insensitive order of their names.
    flows: Vec<SavedFlow>,
    skipped: Vec<SkippedFlow>,
}

impl FlowLibrary {
    /// Reads every flow in `directory`. A missing directory has none; files
    /// that cannot be read are left out and listed in `skipped`.
    pub fn load(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let mut library = Self {
            directory,
            flows: Vec::new(),
            skipped: Vec::new(),
        };

        let entries = match fs::read_dir(&library.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return library,
            Err(error) => {
                library.skipped.push(SkippedFlow {
                    path: library.directory.clone(),
                    error: error.to_string(),
                });
                return library;
            }
        };

        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    library.skipped.push(SkippedFlow {
                        path: library.directory.clone(),
                        error: error.to_string(),
                    });
                    continue;
                }
            };

            if path.extension().and_then(|extension| extension.to_str()) != Some(EXTENSION)
                || !path.is_file()
            {
                continue;
            }

            match read(&path) {
                Ok(flow) => library.flows.push(flow),
                Err(error) => library.skipped.push(SkippedFlow {
                    path,
                    error: error.to_string(),
                }),
            }
        }

        library.sort();
        library
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn flows(&self) -> &[SavedFlow] {
        &self.flows
    }

    /// What loading left out.
    pub fn skipped(&self) -> &[SkippedFlow] {
        &self.skipped
    }

    pub fn get(&self, path: &Path) -> Option<&SavedFlow> {
        self.flows.iter().find(|saved| saved.path == path)
    }

    /// Saves a new flow named `name`, or `name 2`, `name 3` and so on when a
    /// flow's file already has that name. Returns its path.
    pub fn create(&mut self, name: &str, flow: Flow) -> Result<PathBuf, FlowLibraryError> {
        let base = name.trim();
        if !valid_name(base) {
            return Err(FlowLibraryError::InvalidName);
        }

        fs::create_dir_all(&self.directory)?;
        let id = Uuid::new_v4().to_string();
        let file_base = base.replace(['/', '\\', ':'], "-");
        let file_base = match file_base.trim_start_matches('.') {
            "" => "Flow",
            file_base => file_base,
        };

        for number in 1.. {
            let (name, file_name) = if number == 1 {
                (base.to_owned(), file_base.to_owned())
            } else {
                (format!("{base} {number}"), format!("{file_base} {number}"))
            };
            let saved = SavedFlow {
                path: self.directory.join(format!("{file_name}.{EXTENSION}")),
                id: id.clone(),
                name,
                schema_version: 1,
                flow: flow.clone(),
            };
            let content = toml::to_string_pretty(&saved)?;

            let mut file = match fs::File::create_new(&saved.path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };

            if let Err(error) = file.write_all(content.as_bytes()) {
                drop(file);
                let _ = fs::remove_file(&saved.path);
                return Err(error.into());
            }

            let path = saved.path.clone();
            self.flows.push(saved);
            self.sort();
            return Ok(path);
        }

        unreachable!()
    }

    /// Saves a copy of a flow next to it.
    pub fn duplicate(&mut self, path: &Path) -> Result<PathBuf, FlowLibraryError> {
        let saved = self.get(path).ok_or(FlowLibraryError::NotFound)?;
        let name = format!("{} Copy", saved.name);
        let flow = saved.flow.clone();

        self.create(&name, flow)
    }

    /// Saves a flow's blocks and connections, unless its file now holds a
    /// different flow.
    pub fn update(
        &mut self,
        path: &Path,
        expected_id: &str,
        flow: Flow,
    ) -> Result<(), FlowLibraryError> {
        self.change(path, expected_id, |saved| saved.flow = flow)
    }

    /// Renames a flow, unless its file now holds a different flow.
    pub fn rename(
        &mut self,
        path: &Path,
        expected_id: &str,
        name: &str,
    ) -> Result<(), FlowLibraryError> {
        let name = name.trim();
        if !valid_name(name) {
            return Err(FlowLibraryError::InvalidName);
        }

        self.change(path, expected_id, |saved| saved.name = name.to_owned())?;
        self.sort();

        Ok(())
    }

    pub fn delete(&mut self, path: &Path) -> Result<(), FlowLibraryError> {
        let index = self
            .flows
            .iter()
            .position(|saved| saved.path == path)
            .ok_or(FlowLibraryError::NotFound)?;

        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }

        self.flows.remove(index);
        Ok(())
    }

    /// Changes the latest content of a flow's file, as long as it still
    /// holds the expected flow, and writes the file whole.
    fn change(
        &mut self,
        path: &Path,
        expected_id: &str,
        change: impl FnOnce(&mut SavedFlow),
    ) -> Result<(), FlowLibraryError> {
        let saved = self
            .flows
            .iter_mut()
            .find(|saved| saved.path == path)
            .ok_or(FlowLibraryError::NotFound)?;
        if saved.id != expected_id {
            return Err(FlowLibraryError::Replaced);
        }

        let mut updated = read(path)?;
        if updated.id != expected_id {
            return Err(FlowLibraryError::Replaced);
        }

        change(&mut updated);
        write_atomically(path, toml::to_string_pretty(&updated)?.as_bytes())?;
        *saved = updated;

        Ok(())
    }

    fn sort(&mut self) {
        self.flows.sort_by(|a, b| {
            (a.name.to_lowercase(), &a.path).cmp(&(b.name.to_lowercase(), &b.path))
        });
    }
}

/// Moves the flows among `skipped`, the files the collections could not
/// load, into the flows directory: Request Eagle 0.1.22 saved flows inside
/// collections. Other files stay for the collections to report, and so does
/// a flow that cannot be moved now, to move on the next start. Returns where
/// the flows were moved.
pub fn move_flows_out_of_collections<'a>(
    skipped: impl IntoIterator<Item = &'a Path>,
    directory: &Path,
) -> Vec<PathBuf> {
    let mut moved = Vec::new();

    for path in skipped {
        if path.extension().and_then(|extension| extension.to_str()) != Some(EXTENSION) {
            continue;
        }

        // Only whole flows move, never a request, which 0.1.22 also read
        // as one when it had a `flow` table, or a collection's environment
        // that happens to name a variable `flow`.
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let is_flow = source
            .parse::<toml::Table>()
            .is_ok_and(|table| !table.contains_key("request"))
            && toml::from_str::<SavedFlow>(&source).is_ok();
        if !is_flow || fs::create_dir_all(directory).is_err() {
            continue;
        }

        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(destination) = move_flow(path, directory, &stem) {
            moved.push(destination);
        }
    }

    moved
}

/// Moves a flow's file to a free name in `directory`. The new name is a
/// link to the file, which appears whole, never replaces another file and
/// keeps its permissions; across filesystems, a complete copy is linked
/// instead, or renamed where links are not supported, as on FAT drives.
/// The original goes only while it still holds what was moved, so a flow
/// saved meanwhile, or moved by another start at the same time, keeps
/// exactly one copy.
pub(crate) fn move_flow(from: &Path, directory: &Path, stem: &str) -> io::Result<PathBuf> {
    let to = match link_to_free_name(from, directory, stem) {
        Ok(to) => to,
        Err(_) => {
            let temporary = directory.join(format!(".request-eagle-{}.tmp", Uuid::new_v4()));
            let published = fs::copy(from, &temporary).and_then(|_| {
                link_to_free_name(&temporary, directory, stem)
                    .or_else(|_| rename_to_free_name(&temporary, directory, stem))
            });
            let _ = fs::remove_file(&temporary);
            published?
        }
    };

    let unchanged = fs::read(from)
        .and_then(|original| Ok(original == fs::read(&to)?))
        .unwrap_or(false);
    let removed = if unchanged {
        fs::remove_file(from)
    } else {
        Err(io::Error::other("The flow changed while it was moved."))
    };
    if let Err(error) = removed {
        let _ = fs::remove_file(&to);
        return Err(error);
    }

    Ok(to)
}

/// Links `from` as `stem.toml` in `directory`, or `stem 2.toml` and so on
/// when the name is taken.
fn link_to_free_name(from: &Path, directory: &Path, stem: &str) -> io::Result<PathBuf> {
    for to in free_names(directory, stem) {
        match fs::hard_link(from, &to) {
            Ok(()) => return Ok(to),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    unreachable!()
}

/// Renames `from`, a file in `directory`, to the first name that no file
/// has. Unlike a link, a rename replaces a file another start created after
/// the check, so it is only for drives without links.
fn rename_to_free_name(from: &Path, directory: &Path, stem: &str) -> io::Result<PathBuf> {
    let to = free_names(directory, stem)
        .find(|to| !to.exists())
        .expect("names never run out");
    fs::rename(from, &to)?;

    Ok(to)
}

/// `stem.toml`, `stem 2.toml`, `stem 3.toml` and so on in `directory`.
fn free_names(directory: &Path, stem: &str) -> impl Iterator<Item = PathBuf> {
    (1..).map(move |number| match number {
        1 => directory.join(format!("{stem}.{EXTENSION}")),
        number => directory.join(format!("{stem} {number}.{EXTENSION}")),
    })
}

fn read(path: &Path) -> Result<SavedFlow, FlowLibraryError> {
    let source = fs::read_to_string(path)?;
    let mut saved: SavedFlow =
        toml::from_str(&source).map_err(|source| FlowLibraryError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    saved.path = path.to_path_buf();

    Ok(saved)
}

/// Replaces the file whole, so a write that fails midway leaves the previous
/// flow.
fn write_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let temporary = path.with_file_name(format!(".request-eagle-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::File::create_new(&temporary)?;
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

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(char::is_control)
}

#[derive(Debug, Error)]
pub enum FlowLibraryError {
    #[error("Enter a name without control characters.")]
    InvalidName,
    #[error("This flow is not saved anymore.")]
    NotFound,
    #[error("This flow was replaced by a different flow. Your edits have not been saved.")]
    Replaced,
    #[error("Could not read {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("{0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
}
