//! Application preferences and their shared persistence convention.
//! Keybindings have a separate store owned by keybindings_service.

mod agents;
mod appearance;
mod credentials;
mod file;
#[cfg(target_os = "linux")]
mod linux_credentials;
#[cfg(target_os = "macos")]
mod macos_credentials;
#[cfg(feature = "ui")]
mod store;
mod update_track;
#[cfg(windows)]
mod windows_credentials;

pub use agents::{AgentPreferences, ModelPreference, ProviderPreferences};
pub use appearance::{AppearanceMode, AppearancePreferences};
pub use file::{Preferences, PreferencesFile};
pub use request::{
    CertificateFiles, ClientCertificate, HttpVersion, ProxyMode, ProxyPreferences, ProxyProtocol,
    RequestPreferences,
};
#[cfg(feature = "ui")]
pub use store::{
    add_client_certificate, agent_environment, credential_error, init, load, load_error,
    remove_client_certificate, update, update_agent_environment, update_proxy,
};
pub use update_track::UpdateTrack;
