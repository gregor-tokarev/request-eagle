//! Application preferences and their shared persistence convention.
//! Keybindings have a separate store owned by keybindings_service.

mod appearance;
mod credentials;
#[cfg(target_os = "linux")]
mod linux_credentials;
mod request_secrets;
mod store;

#[cfg(test)]
mod tests;

pub use appearance::{AppearanceMode, AppearancePreferences};
pub use request::{HttpVersion, ProxyMode, ProxyPreferences, ProxyProtocol, RequestPreferences};
pub use request_secrets::{read_request_secrets, update_request_secret, write_request_secrets};
pub use store::{Preferences, credential_error, init, load, update, update_proxy};
