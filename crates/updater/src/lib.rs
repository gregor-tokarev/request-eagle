mod cli;
mod install;
mod service;

#[cfg(test)]
mod tests;
#[cfg(test)]
use service::check_for_update;

pub use service::{UpdateManifest, UpdateStatus, Updater, init};

pub use cli::{CliInstaller, CliStatus, cli_path, cli_target};
