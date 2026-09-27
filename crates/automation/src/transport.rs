use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Each application instance owns a separate socket; clients never choose an
/// arbitrary workspace when more than one is running.
pub fn socket_directory() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("REQUEST_EAGLE_AUTOMATION_DIR") {
        return Ok(path.into());
    }

    std::env::home_dir()
        .map(|home| home.join(".request-eagle/automation"))
        .ok_or_else(|| io::Error::other("Cannot locate the home directory"))
}

pub fn prepare_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    validate_directory(path)
}

pub fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "Automation directory must be owned by you, private (0700), and not a symlink",
        ));
    }

    Ok(())
}

pub fn validate_socket(path: &Path) -> io::Result<()> {
    validate_directory(
        path.parent()
            .ok_or_else(|| io::Error::other("Socket needs a parent directory"))?,
    )?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket() || metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(io::Error::other("Expected a local socket owned by you"));
    }

    Ok(())
}
