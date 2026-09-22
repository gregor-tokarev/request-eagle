//! Theme setup shared by Request Eagle windows.

mod appearance;
mod catalog;
mod config;
mod registry;

#[cfg(test)]
mod contrast_tests;
#[cfg(test)]
mod tests;

pub use appearance::apply_preferences;
pub use catalog::{THEME_PAIRS, theme_pair};
pub use config::config;
pub use registry::{apply, init};
