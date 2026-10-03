mod collection;
mod entry;
mod location;
mod order;
mod registry;
#[cfg(feature = "ui")]
mod store;
mod toml_merge;

#[cfg(all(test, feature = "ui"))]
mod store_tests;
#[cfg(test)]
mod tests;

pub use collection::{Collection, CollectionLoadError, CollectionSaveError, SharedSettings};
pub use entry::{DirEntry, Entry, FileEntry};
pub use location::{SavedLocation, directory_name};
pub use registry::{
    CollectionEditError, CollectionRegistry, ImportedCollection, ImportedItem, MovePlacement,
    SkippedPath,
};
#[cfg(feature = "ui")]
pub use store::{Collections, CollectionsEvent, SaveDestination};
