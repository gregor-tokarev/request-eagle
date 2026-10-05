//! Theme setup shared by Request Eagle windows.

mod appearance;
mod keycaps;
mod method;
mod registry;

pub use appearance::apply_preferences;
pub use keycaps::shortcut_keycaps;
pub use method::{method_color, method_label, protocol_icon};
pub use registry::{apply, init, themes};
