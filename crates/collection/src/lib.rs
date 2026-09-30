mod collection;
mod entry;
mod order;
mod registry;
mod toml_merge;

#[cfg(test)]
mod tests;

pub use collection::{Collection, CollectionLoadError, CollectionSaveError};
pub use entry::{DirEntry, Entry, FileEntry};
pub use registry::{
    CollectionEditError, CollectionRegistry, CollectionRegistryLoadError, MovePlacement,
};
