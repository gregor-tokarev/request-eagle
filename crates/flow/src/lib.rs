//! Flows: blocks on a canvas whose connections carry data from one block to
//! the next, as in Postman Flows. The model is saved in collections; the
//! runtime sends their HTTP requests and evaluates their FQL without
//! application or UI state.

mod graph;
mod http;
mod model;
mod runtime;
mod template;

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod template_tests;

pub use http::{SavedRequest, request_variables};
pub use model::{
    Block, BlockKind, BlockType, Connection, DisplayFormat, Field, Flow, TemplateFormat,
    is_identifier,
};
pub use runtime::{BlockRun, MAX_BLOCK_RUNS, RunEvent, RunOptions, RunSummary, run, select};
