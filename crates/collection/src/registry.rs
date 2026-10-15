mod catalog;
mod creation;
mod importing;
mod linking;
mod links;
mod movement;
mod mutations;

pub use catalog::{CollectionRegistry, SkippedPath};
pub(crate) use catalog::{ENVIRONMENT_FILE_NAME, is_hidden};
pub use importing::{ImportedCollection, ImportedItem};
pub use linking::{LinkedFolder, LoadedFrom};
pub use links::FILE_NAME as LINKS_FILE_NAME;
pub use movement::MovePlacement;
pub use mutations::CollectionEditError;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod importing_tests;
#[cfg(test)]
mod linking_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod movement_tests;
