use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::Entry;

const FILE_NAME: &str = ".request-eagle-order.json";

pub(crate) fn apply(parent: &Path, entries: &mut [Entry]) -> io::Result<()> {
    let Some(bytes) = read(parent)? else {
        return Ok(());
    };
    let names: Vec<String> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let positions: HashMap<_, _> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    // Unknown files retain the loader's alphabetical order after saved entries.
    entries.sort_by_key(|entry| {
        positions
            .get(entry.path().file_name().unwrap().to_string_lossy().as_ref())
            .copied()
            .unwrap_or(usize::MAX)
    });
    Ok(())
}

pub(crate) fn save(parent: &Path, paths: &[PathBuf]) -> io::Result<()> {
    let names: Vec<_> = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy())
        .collect();
    write(
        parent,
        &serde_json::to_vec_pretty(&names).map_err(io::Error::other)?,
    )
}

pub(crate) fn with_saved_order<T>(
    parent: &Path,
    paths: &[PathBuf],
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let previous = read(parent)?;
    save(parent, paths)?;
    match operation() {
        Ok(value) => Ok(value),
        Err(error) => {
            let restored = match previous {
                Some(bytes) => write(parent, &bytes),
                None => fs::remove_file(parent.join(FILE_NAME)),
            };
            if let Err(restore_error) = restored {
                return Err(io::Error::other(format!(
                    "{error}; could not restore item order: {restore_error}"
                )));
            }
            Err(error)
        }
    }
}

pub(crate) fn rename_directory(old: &Path, new: &Path) -> io::Result<()> {
    let parent = old
        .parent()
        .ok_or_else(|| io::Error::other("Cannot rename the filesystem root."))?;
    let Some(bytes) = read(parent)? else {
        return fs::rename(old, new);
    };
    let names: Vec<String> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let paths = names
        .iter()
        .map(|name| {
            if Some(std::ffi::OsStr::new(name)) == old.file_name() {
                new.to_path_buf()
            } else {
                parent.join(name)
            }
        })
        .collect::<Vec<_>>();
    with_saved_order(parent, &paths, || fs::rename(old, new))
}

fn read(parent: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(parent.join(FILE_NAME)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write(parent: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = parent.join(format!(".request-eagle-order-{}.tmp", Uuid::new_v4()));
    let result =
        fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, parent.join(FILE_NAME)));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
