//! Theme setup shared by Request Eagle windows.

mod appearance;
mod registry;

#[cfg(test)]
mod contrast_tests;
#[cfg(test)]
mod tests;

pub use appearance::apply_preferences;
pub use registry::{apply, config, init, themes};
