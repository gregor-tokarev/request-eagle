#![recursion_limit = "256"]

mod actions;
mod appearance;
mod cli_access;
mod general;
mod geometry;
mod keybindings;
mod page;
mod proxy;

pub use actions::{CloseSettings, init};
pub use cli_access::CliAccess;
pub use page::{Settings, SettingsEvent, SettingsPage};
