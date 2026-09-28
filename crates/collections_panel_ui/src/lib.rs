#![recursion_limit = "256"]

mod creation;
mod dragging;
mod editing;
mod panel;
mod rows;
mod save_request;
mod search;
mod tree;

#[cfg(test)]
mod editing_tests;
#[cfg(test)]
mod saving_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod tests;

pub use panel::{CollectionPanel, CollectionPanelEvent};

#[cfg(test)]
mod dragging_tests;

pub use save_request::SaveDestination;
