#![recursion_limit = "256"]

mod actions;
mod layout;
mod workspace;

#[cfg(test)]
mod performance;
#[cfg(test)]
mod tests;

pub use actions::{OpenGeneralSettings, OpenSettings};
pub use workspace::init;
