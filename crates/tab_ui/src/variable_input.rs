mod completion;
#[cfg(test)]
mod tests;
mod token;

pub(crate) use completion::{VariableInput, VariableTarget, with_variables};
