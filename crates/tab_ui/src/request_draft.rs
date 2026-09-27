mod body;
mod controls;
mod draft;
mod execution;
mod script_completions;
mod script_signature;
mod scripts;

#[cfg(test)]
mod script_completion_tests;
#[cfg(test)]
mod script_tests;

#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod performance;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod variable_tests;
#[cfg(test)]
mod vim_tests;

pub use draft::RequestDraft;

#[cfg(feature = "test-support")]
mod test_support;
