//! Values during evaluation. Input JSON is borrowed, not copied: a path into
//! a large response costs as much as its steps. Arrays carry JSONata's
//! sequence flags, which decide how results flatten and collapse.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use indexmap::IndexMap;
use serde_json::{Map, Value as Json};

use crate::ast;
use crate::functions::Builtin;

#[derive(Clone)]
pub(crate) enum Value<'a> {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Text<'a>),
    Array(Array<'a>),
    Object(Object<'a>),
    Function(Rc<Function<'a>>),
    Regex(&'a regex::Regex),
    /// An item of a tuple stream: the context and the variables that `@`
    /// and `#` bound for it.
    Tuple(Rc<Tuple<'a>>),
}

#[derive(Clone)]
pub(crate) enum Text<'a> {
    Borrowed(&'a str),
    Owned(Rc<str>),
}

#[derive(Clone)]
pub(crate) struct Array<'a> {
    pub items: Items<'a>,
    /// A sequence of results, which collapses to its only item and flattens
    /// into the sequence of a path.
    pub sequence: bool,
    /// Kept as an array even with one item, by `[]`.
    pub keep_singleton: bool,
    /// Built by an array constructor, which paths do not flatten.
    pub cons: bool,
    /// Wraps an input that is itself an array.
    pub outer_wrapper: bool,
    pub tuple_stream: bool,
}

#[derive(Clone)]
pub(crate) enum Items<'a> {
    Json(&'a [Json]),
    Owned(Rc<Vec<Value<'a>>>),
}

#[derive(Clone)]
pub(crate) enum Object<'a> {
    Json(&'a Map<String, Json>),
    Owned(Rc<IndexMap<String, Value<'a>>>),
}

pub(crate) struct Tuple<'a> {
    pub context: Value<'a>,
    pub bindings: Vec<(String, Value<'a>)>,
}

pub(crate) enum Function<'a> {
    Lambda {
        definition: &'a ast::Lambda,
        environment: Rc<Frame<'a>>,
        input: Value<'a>,
    },
    Builtin(&'static Builtin),
    /// A function with some of its arguments given; `None` are left open.
    Partial {
        function: Value<'a>,
        arguments: Vec<Option<Value<'a>>>,
    },
    /// `$f ~> $g`: calls `$g` with what `$f` returns.
    Chain(Value<'a>, Value<'a>),
    /// `| pattern | update, delete |`, applied to a copy of its argument.
    Transform {
        node: &'a ast::Node,
        environment: Rc<Frame<'a>>,
    },
}

/// Variables bound in a scope, which sees those of its parents.
pub(crate) struct Frame<'a> {
    bindings: RefCell<HashMap<String, Value<'a>>>,
    parent: Option<Rc<Frame<'a>>>,
    /// Whether `:=` assigned a variable here.
    assigned: Cell<bool>,
}

impl<'a> Frame<'a> {
    pub fn root() -> Rc<Self> {
        Rc::new(Self {
            bindings: RefCell::new(HashMap::new()),
            parent: None,
            assigned: Cell::new(false),
        })
    }

    pub fn child(parent: &Rc<Self>) -> Rc<Self> {
        Rc::new(Self {
            bindings: RefCell::new(HashMap::new()),
            parent: Some(parent.clone()),
            assigned: Cell::new(false),
        })
    }

    pub fn bind(&self, name: &str, value: Value<'a>) {
        self.bindings.borrow_mut().insert(name.to_owned(), value);
    }

    /// Bind a variable with `:=`.
    pub fn assign(&self, name: &str, value: Value<'a>) {
        self.assigned.set(true);
        self.bind(name, value);
    }

    pub fn assigned(&self) -> bool {
        self.assigned.get()
    }

    /// Drop the variables, which frees functions that hold this frame.
    pub fn clear(&self) {
        let bindings = self.bindings.take();
        drop(bindings);
    }

    pub fn lookup(&self, name: &str) -> Option<Value<'a>> {
        if let Some(value) = self.bindings.borrow().get(name) {
            return Some(value.clone());
        }

        self.parent.as_ref()?.lookup(name)
    }
}

impl<'a> Text<'a> {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Owned(text) => text,
        }
    }
}

impl<'a> Array<'a> {
    fn new(items: Vec<Value<'a>>, sequence: bool) -> Self {
        Self {
            items: Items::Owned(Rc::new(items)),
            sequence,
            keep_singleton: false,
            cons: false,
            outer_wrapper: false,
            tuple_stream: false,
        }
    }

    pub fn len(&self) -> usize {
        match &self.items {
            Items::Json(items) => items.len(),
            Items::Owned(items) => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<Value<'a>> {
        match &self.items {
            Items::Json(items) => items.get(index).map(Value::from_json),
            Items::Owned(items) => items.get(index).cloned(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Value<'a>> + '_ {
        (0..self.len()).filter_map(|index| self.get(index))
    }

    pub fn to_vec(&self) -> Vec<Value<'a>> {
        self.iter().collect()
    }
}

impl<'a> Object<'a> {
    pub fn get(&self, key: &str) -> Option<Value<'a>> {
        match self {
            Self::Json(object) => object.get(key).map(Value::from_json),
            Self::Owned(object) => object.get(key).cloned(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Json(object) => object.len(),
            Self::Owned(object) => object.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn entries(&self) -> Vec<(String, Value<'a>)> {
        match self {
            Self::Json(object) => object
                .iter()
                .map(|(key, value)| (key.clone(), Value::from_json(value)))
                .collect(),
            Self::Owned(object) => object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }
    }

    pub fn keys(&self) -> Vec<String> {
        match self {
            Self::Json(object) => object.keys().cloned().collect(),
            Self::Owned(object) => object.keys().cloned().collect(),
        }
    }

    pub fn to_map(&self) -> IndexMap<String, Value<'a>> {
        self.entries().into_iter().collect()
    }
}

impl<'a> Value<'a> {
    pub fn from_json(json: &'a Json) -> Self {
        match json {
            Json::Null => Self::Null,
            Json::Bool(value) => Self::Bool(*value),
            Json::Number(number) => Self::Number(number.as_f64().unwrap_or(f64::NAN)),
            Json::String(text) => Self::String(Text::Borrowed(text)),
            Json::Array(items) => Self::Array(Array {
                items: Items::Json(items),
                sequence: false,
                keep_singleton: false,
                cons: false,
                outer_wrapper: false,
                tuple_stream: false,
            }),
            Json::Object(object) => Self::Object(Object::Json(object)),
        }
    }

    pub fn string(text: impl Into<Rc<str>>) -> Self {
        Self::String(Text::Owned(text.into()))
    }

    /// A JSONata sequence of results.
    pub fn sequence(items: Vec<Value<'a>>) -> Self {
        Self::Array(Array::new(items, true))
    }

    /// An array, as a JSON array is.
    pub fn array(items: Vec<Value<'a>>) -> Self {
        Self::Array(Array::new(items, false))
    }

    pub fn object(object: IndexMap<String, Value<'a>>) -> Self {
        Self::Object(Object::Owned(Rc::new(object)))
    }

    pub fn function(function: Function<'a>) -> Self {
        Self::Function(Rc::new(function))
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }

    /// The context to evaluate `input` with: an array is one item, not a
    /// sequence of them.
    pub fn context(input: Self) -> Self {
        match input {
            Self::Array(array) if !array.sequence => {
                let mut wrapper = Self::sequence(vec![Self::Array(array)]);
                if let Self::Array(wrapper) = &mut wrapper {
                    wrapper.outer_wrapper = true;
                }
                wrapper
            }
            input => input,
        }
    }

    pub fn is_function(&self) -> bool {
        matches!(self, Self::Function(_) | Self::Regex(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// The items of an array, or the value as the only item. Undefined has none.
    pub fn items(&self) -> Vec<Value<'a>> {
        match self {
            Self::Undefined => Vec::new(),
            Self::Array(array) => array.to_vec(),
            value => vec![value.clone()],
        }
    }

    /// A sequence's result: nothing, its only item, or the sequence.
    pub fn collapse(self) -> Self {
        match self {
            Self::Array(array) if array.sequence && !array.tuple_stream => match array.len() {
                0 => Self::Undefined,
                1 if !array.keep_singleton => array.get(0).unwrap_or(Self::Undefined),
                _ => Self::Array(array),
            },
            value => value,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) | Self::Tuple(_) => "object",
            Self::Function(_) | Self::Regex(_) => "function",
        }
    }

    /// Whether JSONata counts the value as true, as `$boolean` does.
    /// Undefined is neither.
    pub fn truthy(&self) -> Option<bool> {
        Some(match self {
            Self::Undefined => return None,
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Number(number) => *number != 0.,
            Self::String(text) => !text.as_str().is_empty(),
            Self::Array(array) => match array.len() {
                0 => false,
                1 => array.get(0).and_then(|item| item.truthy()).unwrap_or(false),
                _ => array.iter().any(|item| item.truthy() == Some(true)),
            },
            Self::Object(object) => !object.is_empty(),
            Self::Tuple(_) => true,
            Self::Function(_) | Self::Regex(_) => false,
        })
    }

    /// The value as JSON. Undefined is none, and functions are left out of
    /// objects and are null in arrays.
    pub fn to_json(&self) -> Option<Json> {
        Some(match self {
            Self::Undefined | Self::Function(_) | Self::Regex(_) => return None,
            Self::Null => Json::Null,
            Self::Bool(value) => Json::Bool(*value),
            Self::Number(number) => number_json(*number),
            Self::String(text) => Json::String(text.as_str().to_owned()),
            Self::Array(array) => match &array.items {
                Items::Json(items) => Json::Array(items.to_vec()),
                Items::Owned(items) => Json::Array(
                    items
                        .iter()
                        .map(|item| item.to_json().unwrap_or(Json::Null))
                        .collect(),
                ),
            },
            Self::Object(Object::Json(object)) => Json::Object((*object).clone()),
            Self::Object(Object::Owned(object)) => Json::Object(
                object
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.to_json()?)))
                    .collect(),
            ),
            Self::Tuple(tuple) => tuple.context.to_json()?,
        })
    }
}

/// A number as JSON: whole numbers without a fraction, as JavaScript writes them.
pub(crate) fn number_json(number: f64) -> Json {
    if number.fract() == 0. && number.abs() < 9_007_199_254_740_992. {
        Json::from(number as i64)
    } else {
        serde_json::Number::from_f64(number).map_or(Json::Null, Json::Number)
    }
}

/// Deep equality, as JSONata's `=` compares.
pub(crate) fn equal<'a>(a: &Value<'a>, b: &Value<'a>) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a.as_str() == b.as_str(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| equal(&a, &b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.entries()
                    .iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| equal(value, &other)))
        }
        (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// How JavaScript writes a number, which `$string` and `&` follow.
pub(crate) fn format_number(number: f64) -> String {
    if number == 0. {
        return "0".to_owned();
    }
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number.is_infinite() {
        return if number > 0. { "Infinity" } else { "-Infinity" }.to_owned();
    }

    // The shortest digits that read back as the number, and its exponent.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let count = digits.len() as i32;
    let point = exponent + 1;
    let sign = if number < 0. { "-" } else { "" };

    let text = if count <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - count) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let mantissa = if count == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!(
            "{mantissa}e{}{}",
            if point > 0 { "+" } else { "-" },
            (point - 1).abs()
        )
    };

    format!("{sign}{text}")
}

/// A number rounded to 15 significant digits, as JSONata writes numbers to
/// hide floating point noise such as `0.1 + 0.2`.
pub(crate) fn precise(number: f64) -> f64 {
    if !number.is_finite() || number == 0. {
        return number;
    }
    format!("{number:.14e}").parse().unwrap_or(number)
}
