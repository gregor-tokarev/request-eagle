//! The public API: parsed expressions and the variables they see.

use std::sync::Arc;

use typed_arena::Arena;

use crate::error::Error;
use crate::{ast, evaluator, parser, value};

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
        let expressions = Arena::new();
        let evaluation = evaluator::Evaluation::new(&expressions);
        let environment = value::Frame::root();
        for (name, value) in &bindings.values {
            environment.bind(name, value::Value::from_json(value));
        }

        // `$$` is the input.
        let input = match input {
            None => value::Value::Undefined,
            Some(json) => value::Value::from_json(json),
        };
        environment.bind("$", input.clone());

        evaluation
            .evaluate(&self.root, &value::Value::context(input), &environment)
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
