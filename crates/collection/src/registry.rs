mod catalog;
mod creation;
mod mutations;

pub use catalog::{CollectionRegistry, CollectionRegistryLoadError};
pub use mutations::CollectionEditError;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod tests;
