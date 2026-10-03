use std::{collections::HashMap, sync::OnceLock};

use crate::error::{Error, Result};
use crate::evaluator::Evaluation;
use crate::value::{Object, Value, format_number, precise};

use super::{arrays, dates, numbers, objects, postman, strings};

/// A built-in function, such as `$sum`.
pub struct Builtin {
    /// The name, without `$`.
    pub name: &'static str,
    /// How it is called, such as `$substring(str, start[, length])`.
    pub signature: &'static str,
    pub description: &'static str,
    /// The arguments it needs, counting one the context can give.
    pub(crate) min: usize,
    pub(crate) max: usize,
    /// The first argument defaults to the context, as `-` marks in
    /// JSONata's signatures, so `name.$uppercase()` works.
    pub(crate) context: bool,
    pub(crate) implementation: for<'a, 'e> fn(&Args<'a, 'e>) -> Result<Value<'a>>,
}

/// The arguments of a call to a built-in function.
pub(crate) struct Args<'a, 'e> {
    pub evaluation: &'e Evaluation<'a>,
    pub values: Vec<Value<'a>>,
    pub input: &'e Value<'a>,
    pub position: usize,
    pub name: &'static str,
}

impl<'a> Args<'a, '_> {
    pub fn get(&self, index: usize) -> Value<'a> {
        self.values.get(index).cloned().unwrap_or(Value::Undefined)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn mismatch(&self, index: usize) -> Error {
        Error::at(
            "T0410",
            self.position,
            format!(
                "Argument {} of function \"{}\" does not match function signature",
                index + 1,
                self.name
            ),
        )
    }

    pub fn error(&self, code: &'static str, message: impl Into<String>) -> Error {
        Error::at(code, self.position, message)
    }

    pub fn string(&self, index: usize) -> Result<Option<&str>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.as_str())),
            // A sequence of one string is that string.
            Some(Value::Array(array)) if array.len() == 1 => match array.get(0) {
                Some(Value::String(_)) => Err(self.mismatch(index)),
                _ => Err(self.mismatch(index)),
            },
            Some(_) => Err(self.mismatch(index)),
        }
    }

    pub fn number(&self, index: usize) -> Result<Option<f64>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => Ok(None),
            Some(Value::Number(number)) => Ok(Some(*number)),
            Some(_) => Err(self.mismatch(index)),
        }
    }

    pub fn boolean(&self, index: usize) -> Result<Option<bool>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => Ok(None),
            Some(Value::Bool(value)) => Ok(Some(*value)),
            Some(_) => Err(self.mismatch(index)),
        }
    }

    /// An array argument; a single value counts as an array of it.
    pub fn array(&self, index: usize) -> Option<Vec<Value<'a>>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => None,
            Some(value) => Some(value.items()),
        }
    }

    pub fn numbers(&self, index: usize) -> Result<Option<Vec<f64>>> {
        let Some(items) = self.array(index) else {
            return Ok(None);
        };

        items
            .iter()
            .map(|item| match item {
                Value::Number(number) => Ok(*number),
                _ => Err(self.error(
                    "T0412",
                    format!(
                        "Argument {} of function \"{}\" must be an array of numbers",
                        index + 1,
                        self.name
                    ),
                )),
            })
            .collect::<Result<_>>()
            .map(Some)
    }

    pub fn object(&self, index: usize) -> Result<Option<Object<'a>>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => Ok(None),
            Some(Value::Object(object)) => Ok(Some(object.clone())),
            Some(Value::Tuple(tuple)) => match &tuple.context {
                Value::Object(object) => Ok(Some(object.clone())),
                _ => Err(self.mismatch(index)),
            },
            Some(_) => Err(self.mismatch(index)),
        }
    }

    pub fn function(&self, index: usize) -> Result<Option<Value<'a>>> {
        match self.values.get(index) {
            None | Some(Value::Undefined) => Ok(None),
            Some(value) if value.is_function() => Ok(Some(value.clone())),
            Some(_) => Err(self.mismatch(index)),
        }
    }

    pub fn apply(&self, function: &Value<'a>, arguments: Vec<Value<'a>>) -> Result<Value<'a>> {
        self.evaluation
            .apply(function, arguments, self.input, self.position)
    }

    /// Call a function with as many of `arguments` as it takes, as
    /// higher-order functions do.
    pub fn apply_some(
        &self,
        function: &Value<'a>,
        mut arguments: Vec<Value<'a>>,
    ) -> Result<Value<'a>> {
        arguments.truncate(self.evaluation.arity(function).max(1));
        self.apply(function, arguments)
    }
}

fn table() -> &'static HashMap<&'static str, &'static Builtin> {
    static TABLE: OnceLock<HashMap<&'static str, &'static Builtin>> = OnceLock::new();

    TABLE.get_or_init(|| {
        strings::FUNCTIONS
            .iter()
            .chain(numbers::FUNCTIONS)
            .chain(arrays::FUNCTIONS)
            .chain(objects::FUNCTIONS)
            .chain(dates::FUNCTIONS)
            .chain(postman::FUNCTIONS)
            .map(|builtin| (builtin.name, builtin))
            .collect()
    })
}

pub(crate) fn find(name: &str) -> Option<&'static Builtin> {
    table().get(name).copied()
}

/// Every built-in function, by name, for completion and documentation.
pub fn functions() -> Vec<&'static Builtin> {
    let mut functions: Vec<_> = table().values().copied().collect();
    functions.sort_by_key(|builtin| builtin.name);
    functions
}

pub(crate) fn call<'a>(
    evaluation: &Evaluation<'a>,
    builtin: &'static Builtin,
    mut values: Vec<Value<'a>>,
    input: &Value<'a>,
    position: usize,
) -> Result<Value<'a>> {
    // The context fills a missing first argument: one too few are given,
    // or they only fit the parameters after the first.
    let missing_first = values.len() < builtin.min
        || values.len() < builtin.max
            && parameters(builtin.name).is_some_and(|parameters| {
                let symbols: Vec<char> = values.iter().map(symbol).collect();
                !fits(parameters, &symbols) && fits(&parameters[1..], &symbols)
            });
    if builtin.context && missing_first {
        let context = match input {
            Value::Array(array) if array.outer_wrapper => array.get(0).unwrap_or(Value::Undefined),
            input => input.clone(),
        };
        values.insert(0, context);
    }

    if values.len() > builtin.max {
        return Err(Error::at(
            "T0410",
            position,
            format!(
                "Argument {} of function \"{}\" does not match function signature",
                builtin.max + 1,
                builtin.name
            ),
        ));
    }

    (builtin.implementation)(&Args {
        evaluation,
        values,
        input,
        position,
        name: builtin.name,
    })
}

/// The parameters of the functions whose first argument the context can
/// fill while they take others, from JSONata's signatures: the types each
/// accepts, with `?` after optional ones.
fn parameters(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "string" | "json" => &["x", "b?"],
        "substring" => &["s", "n", "n?"],
        "substringBefore" | "substringAfter" | "parseInteger" => &["s", "s"],
        "pad" => &["s", "n", "s?"],
        "contains" => &["s", "sf"],
        "split" | "match" => &["s", "sf", "n?"],
        "replace" => &["s", "sf", "sf", "n?"],
        "round" | "formatBase" => &["n", "n?"],
        "power" => &["n", "n"],
        "formatNumber" => &["n", "s", "o?"],
        "formatInteger" => &["n", "s"],
        "lookup" => &["x", "s"],
        "sift" => &["o", "f?"],
        "each" => &["o", "f"],
        "fromMillis" => &["n", "s?", "s?"],
        "toMillis" => &["s", "s?"],
        _ => return None,
    })
}

/// Whether arguments of these types fit the parameters. Undefined fits any.
fn fits(parameters: &[&str], symbols: &[char]) -> bool {
    let Some((parameter, rest)) = parameters.split_first() else {
        return symbols.is_empty();
    };
    let (types, optional) = match parameter.strip_suffix('?') {
        Some(types) => (types, true),
        None => (*parameter, false),
    };

    symbols.split_first().is_some_and(|(&symbol, others)| {
        (symbol == 'm' || types == "x" || types.contains(symbol)) && fits(rest, others)
    }) || optional && fits(rest, symbols)
}

/// The type of a value, as JSONata's signatures write it.
fn symbol(value: &Value) -> char {
    match value {
        Value::Undefined => 'm',
        Value::Null => 'l',
        Value::Bool(_) => 'b',
        Value::Number(_) => 'n',
        Value::String(_) => 's',
        Value::Array(_) => 'a',
        Value::Object(_) | Value::Tuple(_) => 'o',
        Value::Function(_) | Value::Regex(_) => 'f',
    }
}

/// A value as text, as `$string` writes it: strings as they are, other
/// values as JSON with numbers rounded to 15 significant digits.
pub(crate) fn stringify(value: &Value, pretty: bool) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.as_str().to_owned()),
        Value::Function(_) | Value::Regex(_) => Ok(String::new()),
        Value::Number(number) if !number.is_finite() => Err(Error::new(
            "D3001",
            "Attempting to invoke string function on Infinity or NaN",
        )),
        value => {
            let mut output = String::new();
            write_json(value, pretty, 0, &mut output);
            Ok(output)
        }
    }
}

fn write_json(value: &Value, pretty: bool, depth: usize, output: &mut String) {
    let indent = |output: &mut String, depth: usize| {
        if pretty {
            output.push('\n');
            output.push_str(&"  ".repeat(depth));
        }
    };

    match value {
        Value::Undefined | Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            if number.is_finite() {
                output.push_str(&format_number(precise(*number)));
            } else {
                output.push_str("null");
            }
        }
        Value::String(text) => {
            output.push_str(&serde_json::to_string(text.as_str()).unwrap_or_default())
        }
        Value::Function(_) | Value::Regex(_) => output.push_str("\"\""),
        Value::Tuple(tuple) => write_json(&tuple.context, pretty, depth, output),
        Value::Array(array) => {
            if array.is_empty() {
                output.push_str("[]");
                return;
            }
            output.push('[');
            for (index, item) in array.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                indent(output, depth + 1);
                write_json(&item, pretty, depth + 1, output);
            }
            indent(output, depth);
            output.push(']');
        }
        Value::Object(object) => {
            let entries: Vec<_> = object
                .entries()
                .into_iter()
                .filter(|(_, value)| !value.is_undefined())
                .collect();
            if entries.is_empty() {
                output.push_str("{}");
                return;
            }
            output.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                indent(output, depth + 1);
                output.push_str(&serde_json::to_string(key).unwrap_or_default());
                output.push_str(if pretty { ": " } else { ":" });
                write_json(value, pretty, depth + 1, output);
            }
            indent(output, depth);
            output.push('}');
        }
    }
}
