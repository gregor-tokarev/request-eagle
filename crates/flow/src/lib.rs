//! Flows: blocks on a canvas whose connections carry data from one block to
//! the next, as in Postman Flows. The library saves them in their own
//! directory, apart from collections; the runtime sends the collections'
//! HTTP requests and evaluates their FQL without application or UI state.

mod graph;
mod http;
mod library;
mod model;
mod runtime;
mod start_input;
mod template;

#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod start_input_tests;
#[cfg(test)]
mod template_tests;

pub use http::{SavedRequest, request_variables};
pub use library::{
    FlowLibrary, FlowLibraryError, SavedFlow, SkippedFlow, move_flows_out_of_collections,
};
pub use model::{
    Block, BlockKind, BlockType, Connection, DisplayFormat, Field, Flow, TemplateFormat,
    is_identifier,
};
pub use runtime::{BlockRun, MAX_BLOCK_RUNS, RunEvent, RunOptions, RunSummary, run, select};
pub use start_input::{JsonError, StartInput, parse_start_input};
