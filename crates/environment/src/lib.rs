mod environment;
mod registry;
mod session;
mod variables;

#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod variable_tests;

pub use environment::{Environment, EnvironmentLoadError, EnvironmentSaveError};
pub use registry::EnvironmentRegistry;
pub use session::{EnvironmentSession, EnvironmentSessions};
pub use variables::{
    GENERATED_VARIABLES, VariableError, VariableResolver, VariableValues, generate_variable,
    valid_variable_name,
};
