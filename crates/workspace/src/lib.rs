#![recursion_limit = "256"]

mod actions;
mod bottom_panel;
mod command_palette;
mod environment_panel;
mod environment_picker;
mod flow_panel;
mod history_panel;
mod main_view;
mod save_request;
mod session;
mod top_panel;
mod workspace;

#[cfg(test)]
mod actions_tests;
#[cfg(test)]
mod history_panel_tests;
#[cfg(test)]
mod session_tests;

pub use actions::{OpenGeneralSettings, OpenSettings};
pub use session::Session;
pub use workspace::init;
