//! Application preferences and their shared persistence convention.
//! Keybindings have a separate store owned by keybindings_service.

mod appearance;
mod credentials;
mod file;
#[cfg(target_os = "linux")]
mod linux_credentials;
#[cfg(target_os = "macos")]
mod macos_credentials;
#[cfg(feature = "ui")]
mod store;

pub use appearance::{AppearanceMode, AppearancePreferences};
pub use file::{Preferences, PreferencesFile, UpdateChannel};
pub use request::{
    CertificateFiles, ClientCertificate, HttpVersion, ProxyMode, ProxyPreferences, ProxyProtocol,
    RequestPreferences,
};
#[cfg(feature = "ui")]
pub use store::{
    add_client_certificate, credential_error, init, load, remove_client_certificate, update,
    update_proxy,
};
