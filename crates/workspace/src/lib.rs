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

#[cfg(test)]
mod collection_tab_tests;
#[cfg(test)]
mod command_palette_tests;
#[cfg(test)]
mod environment_tests;
#[cfg(test)]
mod main_view_tests;
#[cfg(test)]
mod performance;
#[cfg(test)]
mod request_shortcut_tests;
#[cfg(test)]
mod request_tab_tests;
#[cfg(test)]
mod response_tests;
#[cfg(test)]
mod save_request_tests;
#[cfg(test)]
mod tests;

pub use actions::{OpenGeneralSettings, OpenSettings};
pub use workspace::init;
