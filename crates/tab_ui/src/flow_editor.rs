//! The flow canvas: blocks connected like Postman Flows, edited with the
//! pointer and the keyboard, and run with the active environment.

mod actions;
mod blocks;
mod canvas;
mod editing;
mod editor;
mod geometry;
mod history;
mod inspector;
mod picker;
mod preview;
mod run;
mod run_log;
mod zoom;

#[cfg(test)]
mod editing_tests;
#[cfg(test)]
mod geometry_tests;
#[cfg(test)]
mod inspector_tests;
#[cfg(test)]
mod preview_tests;

pub use actions::*;
pub use editor::{FlowEditor, FlowRequest, FlowRequests};
