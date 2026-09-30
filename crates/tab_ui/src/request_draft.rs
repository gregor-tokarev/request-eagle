mod body;
mod controls;
mod draft;
mod execution;
mod fields;

#[cfg(test)]
pub(crate) mod script_completion_tests;
#[cfg(test)]
mod script_tests;

#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod performance;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod variable_tests;
#[cfg(test)]
mod vim_tests;

pub use draft::{RequestDraft, RequestLocation};

#[cfg(any(test, feature = "test-support"))]
mod test_support;
