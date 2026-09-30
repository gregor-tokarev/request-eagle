mod actions;
mod appearance;
mod general;
mod keybindings;
mod layout;
mod page;
mod proxy;

#[cfg(test)]
mod proxy_tests;

pub use actions::{CloseSettings, init};
pub use page::{Settings, SettingsEvent, SettingsPage};
