//! FQL, the query and transformation language of Postman Flows: JSONata
//! with Postman's additions. Flows' Evaluate, If and Condition blocks use it,
//! with their variables as fields of the input.
//!
//! ```
//! let input = serde_json::json!({"orders": [{"price": 2, "quantity": 3}]});
//! let total = fql::evaluate("$sum(orders.(price * quantity))", Some(&input), &Default::default());
//! assert_eq!(total, Ok(Some(serde_json::json!(6))));
//! ```
//!
//! Expressions parse once and evaluate many times. Evaluation borrows the
//! input instead of copying it, stops expressions that nest too deeply or run
//! too long with an error, and returns `None` for an undefined result.

mod ast;
mod error;
mod evaluator;
mod expression;
mod functions;
mod lexer;
mod parser;
mod patterns;
mod value;

#[cfg(test)]
mod dates_tests;
#[cfg(test)]
mod evaluator_tests;
#[cfg(test)]
mod functions_tests;
#[cfg(test)]
mod parser_tests;

pub use error::Error;
pub use expression::{Bindings, Expression, evaluate};
pub use functions::{Builtin as Function, functions};
