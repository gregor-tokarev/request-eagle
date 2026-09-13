#![recursion_limit = "256"]

mod actions;
mod appearance;
mod general;
mod keybindings;
mod page;

pub use actions::{CloseSettings, init};
pub use page::{Settings, SettingsEvent, SettingsPage};
