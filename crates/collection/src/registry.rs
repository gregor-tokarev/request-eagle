mod catalog;
mod creation;
mod movement;
mod mutations;

pub use catalog::{CollectionRegistry, CollectionRegistryLoadError};
pub use movement::MovePlacement;
pub use mutations::CollectionEditError;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod movement_tests;
