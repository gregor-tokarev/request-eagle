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

use std::sync::Arc;

pub use error::Error;
pub use functions::{Builtin as Function, functions};

/// A parsed expression.
#[derive(Clone, Debug)]
pub struct Expression {
    root: Arc<ast::Node>,
}

impl Expression {
    pub fn parse(source: &str) -> Result<Self, Error> {
        parser::parse(source).map(|root| Self {
            root: Arc::new(root),
        })
    }

    /// Evaluate with `input` as the context `$`, and `bindings` as `$name`
    /// variables. An undefined result is `None`.
    pub fn evaluate(
        &self,
        input: Option<&serde_json::Value>,
        bindings: &Bindings,
    ) -> Result<Option<serde_json::Value>, Error> {
        let evaluation = evaluator::Evaluation::new();
        let environment = value::Frame::root();
        for (name, value) in &bindings.values {
            environment.bind(name, value::Value::from_json(value));
        }

        // `$$` is the input; an input that is an array is evaluated as one
        // item, not item by item.
        let input = match input {
            None => value::Value::Undefined,
            Some(json) => value::Value::from_json(json),
        };
        environment.bind("$", input.clone());
        let context = match input {
            value::Value::Array(array) => {
                let mut wrapper = value::Value::sequence(vec![value::Value::Array(array)]);
                if let value::Value::Array(wrapper) = &mut wrapper {
                    wrapper.outer_wrapper = true;
                }
                wrapper
            }
            input => input,
        };

        evaluation
            .evaluate(&self.root, &context, &environment)
            .map(|result| result.to_json())
    }
}

/// Variables bound before evaluation, visible as `$name`.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    values: Vec<(String, serde_json::Value)>,
}

impl Bindings {
    pub fn insert(&mut self, name: impl Into<String>, value: serde_json::Value) {
        let name = name.into();
        match self
            .values
            .iter_mut()
            .find(|(existing, _)| *existing == name)
        {
            Some((_, existing)) => *existing = value,
            None => self.values.push((name, value)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&serde_json::Value> {
        self.values
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, value)| value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
}

impl FromIterator<(String, serde_json::Value)> for Bindings {
    fn from_iter<T: IntoIterator<Item = (String, serde_json::Value)>>(iter: T) -> Self {
        let mut bindings = Self::default();
        for (name, value) in iter {
            bindings.insert(name, value);
        }
        bindings
    }
}

/// Parse and evaluate `source`.
pub fn evaluate(
    source: &str,
    input: Option<&serde_json::Value>,
    bindings: &Bindings,
) -> Result<Option<serde_json::Value>, Error> {
    Expression::parse(source)?.evaluate(input, bindings)
}
