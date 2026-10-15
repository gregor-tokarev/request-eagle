use std::{
    fs, io,
    path::{Path, PathBuf},
};

use uuid::Uuid;

/// Lists the folders linked as collections on this device. It belongs in
/// the data directory rather than the collections directory, so that
/// syncing the collections does not take this device's folders elsewhere.
pub const FILE_NAME: &str = "linked-collections.json";

/// The linked folders, in the order they were linked. A missing file lists
/// none.
pub(super) fn read(path: &Path) -> io::Result<Vec<PathBuf>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub(super) fn write(path: &Path, folders: &[PathBuf]) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(folders).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary = path.with_file_name(format!(".request-eagle-links-{}.tmp", Uuid::new_v4()));
    let result = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }

    result
}
