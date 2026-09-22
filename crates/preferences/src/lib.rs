//! Application preferences and their shared persistence convention.
//! Keybindings have a separate store owned by keybindings_service.

mod appearance;
mod store;

#[cfg(test)]
mod tests;

pub use appearance::{AppearanceMode, AppearancePreferences};
pub use request::{HttpVersion, ProxyMode, ProxyPreferences, ProxyProtocol, RequestPreferences};
pub use store::{Preferences, init, load, update};
