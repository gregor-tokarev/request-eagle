mod install;
mod service;

#[cfg(test)]
mod tests;

pub use install::confirm_startup;
pub use service::{UpdateManifest, UpdateStatus, Updater, init};
