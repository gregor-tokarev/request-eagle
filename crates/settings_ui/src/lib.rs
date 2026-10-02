mod actions;
mod appearance;
mod certificates;
mod general;
mod keybindings;
mod layout;
mod page;
mod proxy;

pub use actions::{CloseSettings, init};
pub use page::{Settings, SettingsEvent, SettingsPage};
