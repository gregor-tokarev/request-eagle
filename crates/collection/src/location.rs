use std::path::{Path, PathBuf};

use crate::registry::ENVIRONMENT_FILE_NAME;

/// Where a saved request is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLocation {
    /// The request's file.
    pub path: PathBuf,
    /// Tells the request apart from another one saved at the same path later.
    pub id: String,
    pub name: String,
    /// The directory of the collection that stores the request.
    pub collection: PathBuf,
}

impl SavedLocation {
    /// The collection's name, which is its directory's name.
    pub fn collection_name(&self) -> String {
        directory_name(&self.collection)
    }

    /// The folders between the collection and the request, outermost first.
    pub fn folders(&self) -> Vec<String> {
        self.path
            .strip_prefix(&self.collection)
            .ok()
            .and_then(Path::parent)
            .into_iter()
            .flat_map(Path::iter)
            .map(|folder| folder.to_string_lossy().into_owned())
            .collect()
    }

    /// The collection's variables, which the request's variables resolve from.
    pub fn environment_path(&self) -> PathBuf {
        self.collection.join(ENVIRONMENT_FILE_NAME)
    }
}

/// A collection's or folder's name, which is its directory's name.
pub fn directory_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
