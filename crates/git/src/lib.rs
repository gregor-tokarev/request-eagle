mod command;
mod repository;
mod status;

#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod status_tests;

pub use command::GitError;
pub use repository::{Repository, real_path};
pub use status::{Change, ChangeKind, Interrupted, Status};
