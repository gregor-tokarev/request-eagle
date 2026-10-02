#[cfg(target_os = "macos")]
mod install;
mod package;
mod release_notes;
mod service;

#[cfg(target_os = "macos")]
pub use install::confirm_startup;
pub use package::INSTALLS_IN_APP;
#[cfg(not(target_os = "macos"))]
pub use package::confirm_startup;
pub use release_notes::{Change, ReleaseNotes, release_notes};
pub use service::{UpdateManifest, UpdateStatus, Updater, init};
