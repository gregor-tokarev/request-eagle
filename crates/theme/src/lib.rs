//! Theme setup shared by Request Eagle windows.

mod appearance;
mod method;
mod registry;

pub use appearance::apply_preferences;
pub use method::method_color;
pub use registry::{apply, init, themes};
