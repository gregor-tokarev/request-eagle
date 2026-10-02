#[cfg(target_os = "macos")]
mod install;
mod package;
mod release_notes;
mod service;

pub use package::INSTALLS_IN_APP;
pub use release_notes::{Change, ReleaseNotes, release_notes};
pub use service::{UpdateManifest, UpdateStatus, Updater, init};
