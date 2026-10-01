mod body;
mod controls;
mod draft;
mod execution;
mod fields;

#[cfg(test)]
mod header_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod variable_tests;

pub(crate) use controls::request_header;
pub use draft::{RequestDraft, RequestLocation};
pub(crate) use fields::{FieldsChanged, RequestFields};
