#![recursion_limit = "256"]

mod actions;
mod history;
mod imports;
mod layout;
mod session;
mod window_options;
mod workspace;

#[cfg(test)]
mod performance;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod session_writer_tests;
#[cfg(test)]
mod test_allocator;
#[cfg(test)]
mod tests;

pub use actions::{OpenGeneralSettings, OpenSettings};
pub use workspace::init;
