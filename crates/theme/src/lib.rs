//! Theme setup shared by Request Eagle windows.

mod appearance;
mod method;
mod registry;

#[cfg(test)]
mod contrast_tests;
#[cfg(test)]
mod tests;

pub use appearance::apply_preferences;
pub use method::method_color;
pub use registry::{apply, init, themes};
