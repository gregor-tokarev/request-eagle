#![recursion_limit = "256"]

mod actions;
mod bottom_panel;
mod command_palette;
mod environment_panel;
mod environment_picker;
mod main_view;
mod save_request;
mod top_panel;
mod workspace;

pub use actions::{OpenGeneralSettings, OpenSettings};
pub use workspace::init;
