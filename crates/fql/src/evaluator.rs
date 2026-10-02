//! Evaluates parsed expressions with JSONata's semantics, following
//! jsonata-js: paths map over sequences and flatten them, sequences of one
//! item collapse to it, and undefined is the empty sequence.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use indexmap::IndexMap;
use typed_arena::Arena;

use crate::ast::{Group, Kind, Lambda, Node, Operator};
use crate::error::{Error, Result};
use crate::functions;
use crate::value::{Frame, Function, Items, Object, Tuple, Value, equal, format_number};

/// How deeply evaluation may nest, which stops a recursive function that
/// never returns. Tail calls do not count.
const MAX_DEPTH: usize = 3000;

/// Evaluation grows the stack by this much when less than `STACK_RED_ZONE`
/// is left, so deep nesting cannot overflow a thread's stack.
const STACK_SEGMENT: usize = 2 * 1024 * 1024;
const STACK_RED_ZONE: usize = 256 * 1024;

/// How many expressions one evaluation may evaluate, which stops one that
/// never ends.
const MAX_STEPS: u64 = 5_000_000;

/// A function's result, or a call it makes last, which `apply` makes
/// without nesting deeper.
pub(crate) enum Outcome<'a> {
    Value(Value<'a>),
    TailCall(Value<'a>, Vec<Value<'a>>),
}

pub(crate) struct Evaluation<'a> {
    steps: Cell<u64>,
    depth: Cell<usize>,
    /// The time `$now()` and `$millis()` return, the same for the whole evaluation.
    pub now: chrono::DateTime<chrono::Utc>,
    random: Cell<u64>,
    /// Expressions that `$eval` parsed, which live as long as the evaluation.
    expressions: &'a Arena<Node>,
    /// Each expression `$eval` parsed, by its text, so repeating one parses
    /// it once.
    parsed: RefCell<HashMap<String, &'a Node>>,
    /// The scope of each function call under way, innermost last.
    scopes: RefCell<Vec<Rc<Frame<'a>>>>,
    /// Scopes with assigned variables that are still held when they end,
    /// such as by a function assigned in them that holds them in turn. They
    /// are cleared when the evaluation ends, which frees such cycles.
    held: RefCell<Vec<Rc<Frame<'a>>>>,
}

impl Drop for Evaluation<'_> {
    fn drop(&mut self) {
        for frame in self.held.take() {
            frame.clear();
        }
    }
}

impl<'a> Evaluation<'a> {
    pub fn new(expressions: &'a Arena<Node>) -> Self {
        let now = chrono::Utc::now();
        let seed = {
            use std::hash::{BuildHasher, Hasher};
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_i64(now.timestamp_nanos_opt().unwrap_or_default());
            hasher.finish() | 1
        };

        Self {
            steps: Cell::new(0),
            depth: Cell::new(0),
            now,
            random: Cell::new(seed),
            expressions,
            parsed: RefCell::new(HashMap::new()),
            scopes: RefCell::new(Vec::new()),
            held: RefCell::new(Vec::new()),
        }
    }

    /// Parse an expression for `$eval`, once for each text.
    pub fn parse(&self, source: &str) -> Result<&'a Node> {
        if let Some(node) = self.parsed.borrow().get(source) {
            return Ok(node);
        }

        let node: &'a Node = self.expressions.alloc(crate::parser::parse(source)?);
        self.parsed.borrow_mut().insert(source.to_owned(), node);
        Ok(node)
    }

    /// End the scope of `frame`. One that a variable was assigned in and
    /// that is still held may be part of a cycle; see `held`.
    fn close(&self, frame: Rc<Frame<'a>>) {
        if frame.assigned() && Rc::strong_count(&frame) > 1 {
            self.held.borrow_mut().push(frame);
        }
    }

    /// The scope of the function call under way, where `$eval` evaluates.
    pub fn scope(&self) -> Option<Rc<Frame<'a>>> {
        self.scopes.borrow().last().cloned()
    }

    /// A random number from 0 up to 1, by xorshift.
    pub fn random(&self) -> f64 {
        let mut state = self.random.get();
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random.set(state);
        (state >> 11) as f64 / (1u64 << 53) as f64
    }

    fn enter(&self, position: usize) -> Result<()> {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        if steps > MAX_STEPS {
            return Err(Error::at(
                "D1012",
                position,
                "Expression evaluation timeout: check for an expression that never ends",
            ));
        }

        let depth = self.depth.get() + 1;
        self.depth.set(depth);
        if depth > MAX_DEPTH {
            self.depth.set(depth - 1);
            return Err(Error::at(
                "D1011",
                position,
                "Stack overflow error: check for a non-terminating recursive function; tail calls do not nest",
            ));
        }

        Ok(())
    }

    fn leave(&self) {
        self.depth.set(self.depth.get() - 1);
    }

    pub fn evaluate(
        &self,
        node: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
            self.evaluate_node(node, input, env)
        })
    }

    fn evaluate_node(
        &self,
        node: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        self.enter(node.position)?;
        let result = self.evaluate_kind(node, input, env);
        self.leave();
        let mut result = result.map_err(|error| error.or_at(node.position))?;

        for predicate in &node.predicates {
            result = self.filter(predicate, result, env)?;
        }
        if !matches!(node.kind, Kind::Path { .. })
            && let Some(group) = &node.group
        {
            result = self.group(group, result, env)?;
        }

        if node.keep_array
            && let Value::Array(array) = &mut result
            && array.sequence
        {
            array.keep_singleton = true;
        }

        Ok(result.collapse())
    }

    fn evaluate_kind(
        &self,
        node: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        Ok(match &node.kind {
            Kind::Path {
                steps,
                keep_singleton,
            } => self.path(node, steps, *keep_singleton, input, env)?,
            Kind::Name(name) => lookup(input, name),
            Kind::String(text) => Value::String(crate::value::Text::Borrowed(text)),
            Kind::Number(number) => Value::Number(*number),
            Kind::Bool(value) => Value::Bool(*value),
            Kind::Null => Value::Null,
            Kind::Variable(name) => match name.as_str() {
                "" => match input {
                    Value::Array(array) if array.outer_wrapper => {
                        array.get(0).unwrap_or(Value::Undefined)
                    }
                    input => input.clone(),
                },
                name => env
                    .lookup(name)
                    .or_else(|| {
                        functions::find(name)
                            .map(|builtin| Value::function(Function::Builtin(builtin)))
                    })
                    .unwrap_or(Value::Undefined),
            },
            Kind::Wildcard => wildcard(input),
            Kind::Descendants => descendants(input),
            Kind::Regex(regex) => Value::Regex(regex),
            Kind::Negate(operand) => match self.evaluate(operand, input, env)? {
                Value::Undefined => Value::Undefined,
                Value::Number(number) => Value::Number(-number),
                _ => {
                    return Err(Error::at(
                        "D1002",
                        node.position,
                        "Cannot negate a non-numeric value",
                    ));
                }
            },
            Kind::Array(items) => {
                let mut values = Vec::new();
                for item in items {
                    match &item.kind {
                        Kind::Range(start, end) => {
                            let range = self.range(start, end, input, env)?;
                            values.extend(range);
                        }
                        // A nested array constructor stays one item.
                        Kind::Array(_) => {
                            let value = self.evaluate(item, input, env)?;
                            if !value.is_undefined() {
                                values.push(value);
                            }
                        }
                        _ => {
                            let value = self.evaluate(item, input, env)?;
                            match value {
                                Value::Undefined => {}
                                Value::Array(array) => values.extend(array.iter()),
                                value => values.push(value),
                            }
                        }
                    }
                }
                let mut array = Value::array(values);
                if node.cons_array
                    && let Value::Array(array) = &mut array
                {
                    array.cons = true;
                }
                array
            }
            Kind::Range(start, end) => Value::array(self.range(start, end, input, env)?),
            Kind::Object(pairs) => self.group_pairs(pairs, input.clone(), env)?,
            Kind::Binary { operator, lhs, rhs } => {
                self.binary(*operator, lhs, rhs, input, env, node.position)?
            }
            Kind::Condition {
                condition,
                then,
                otherwise,
            } => {
                let condition = self.evaluate(condition, input, env)?;
                if condition.truthy() == Some(true) {
                    self.evaluate(then, input, env)?
                } else if let Some(otherwise) = otherwise {
                    self.evaluate(otherwise, input, env)?
                } else {
                    Value::Undefined
                }
            }
            Kind::Block(expressions) => {
                let frame = Frame::child(env);
                let mut result = Ok(Value::Undefined);
                for expression in expressions {
                    result = self.evaluate(expression, input, &frame);
                    if result.is_err() {
                        break;
                    }
                }
                self.close(frame);
                result?
            }
            Kind::Bind { name, value } => {
                let value = self.evaluate(value, input, env)?;
                env.assign(name, value.clone());
                value
            }
            Kind::Call {
                procedure,
                arguments,
            } => self.call(procedure, arguments, input, env, None, node.position)?,
            Kind::Placeholder => Value::Undefined,
            Kind::Lambda(definition) => Value::function(Function::Lambda {
                definition,
                environment: env.clone(),
                input: input.clone(),
            }),
            Kind::Apply { lhs, rhs } => self.apply_operator(lhs, rhs, input, env, node.position)?,
            Kind::Sort(terms) => self.sort(terms, input.items(), env, node.position)?,
            Kind::IndexBind(_) => Value::Undefined,
            Kind::Transform { .. } => Value::function(Function::Transform {
                node,
                environment: env.clone(),
            }),
        })
    }

    /// Evaluate a function's body, returning a call it makes last instead of
    /// making it, so recursion in tail position does not nest.
    fn evaluate_tail(
        &self,
        node: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Outcome<'a>> {
        if !node.is_plain() {
            return self.evaluate(node, input, env).map(Outcome::Value);
        }

        match &node.kind {
            Kind::Call {
                procedure,
                arguments,
            } if !arguments
                .iter()
                .any(|argument| matches!(argument.kind, Kind::Placeholder)) =>
            {
                let function = self.evaluate(procedure, input, env)?;
                if !matches!(&function, Value::Function(function) if matches!(**function, Function::Lambda { .. }))
                {
                    return self
                        .call_with(
                            function,
                            procedure,
                            arguments,
                            input,
                            env,
                            None,
                            node.position,
                        )
                        .map(Outcome::Value);
                }
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(self.evaluate(argument, input, env)?);
                }
                Ok(Outcome::TailCall(function, values))
            }
            Kind::Condition {
                condition,
                then,
                otherwise,
            } => {
                let condition = self.evaluate(condition, input, env)?;
                if condition.truthy() == Some(true) {
                    self.evaluate_tail(then, input, env)
                } else if let Some(otherwise) = otherwise {
                    self.evaluate_tail(otherwise, input, env)
                } else {
                    Ok(Outcome::Value(Value::Undefined))
                }
            }
            Kind::Block(expressions) if !expressions.is_empty() => {
                let frame = Frame::child(env);
                let (last, first) = expressions.split_last().expect("not empty");
                let outcome = first
                    .iter()
                    .try_for_each(|expression| self.evaluate(expression, input, &frame).map(drop))
                    .and_then(|()| self.evaluate_tail(last, input, &frame));
                self.close(frame);
                outcome
            }
            _ => self.evaluate(node, input, env).map(Outcome::Value),
        }
    }

    fn path(
        &self,
        node: &'a Node,
        steps: &'a [Node],
        keep_singleton: bool,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        // A path that starts with a variable reads it, not each input item.
        let mut items = match input {
            Value::Array(array) if !matches!(steps[0].kind, Kind::Variable(_)) => array.to_vec(),
            Value::Undefined => Vec::new(),
            input => vec![input.clone()],
        };
        let mut result = Value::sequence(Vec::new());
        let mut tuples: Option<Vec<Rc<Tuple<'a>>>> = None;

        for (index, step) in steps.iter().enumerate() {
            if step.tuple && tuples.is_none() {
                tuples = Some(
                    items
                        .iter()
                        .map(|item| {
                            Rc::new(Tuple {
                                context: item.clone(),
                                bindings: Vec::new(),
                            })
                        })
                        .collect(),
                );
            }

            if let Some(current) = tuples.take() {
                let next = self.tuple_step(step, current, env)?;
                tuples = Some(next);
                continue;
            }

            result = if index == 0 && step.cons_array {
                // A path that starts with an array constructor uses it whole.
                self.evaluate(step, &Value::sequence(items.clone()), env)?
            } else {
                self.step(step, &items, env, index == steps.len() - 1)?
            };

            match &result {
                Value::Undefined => break,
                Value::Array(array) if array.is_empty() => break,
                _ => {}
            }
            if step.focus.is_none() {
                items = result.items();
            }
        }

        if let Some(tuples) = tuples {
            result = Value::sequence(tuples.iter().map(|tuple| tuple.context.clone()).collect());
            if let Some(group) = &node.group {
                return self.group_tuples(group, tuples, env);
            }
        }

        if keep_singleton {
            result = match result {
                Value::Array(array) if array.cons && !array.sequence => {
                    Value::sequence(vec![Value::Array(array)])
                }
                Value::Undefined => Value::Undefined,
                Value::Array(array) => Value::Array(array),
                value => Value::sequence(vec![value]),
            };
            if let Value::Array(array) = &mut result {
                array.keep_singleton = true;
            }
        }

        if let Some(group) = &node.group {
            result = self.group(group, result, env)?;
        }

        Ok(result)
    }

    fn step(
        &self,
        step: &'a Node,
        items: &[Value<'a>],
        env: &Rc<Frame<'a>>,
        last: bool,
    ) -> Result<Value<'a>> {
        if let Kind::Sort(terms) = &step.kind {
            let mut result = self.sort(terms, items.to_vec(), env, step.position)?;
            for stage in &step.stages {
                result = self.filter(stage, result, env)?;
            }
            return Ok(result);
        }

        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let mut value = self.evaluate(step, item, env)?;
            for stage in &step.stages {
                value = self.filter(stage, value, env)?;
            }
            if !value.is_undefined() {
                results.push(value);
            }
        }

        // The last step keeps an array it produced on its own.
        if last
            && results.len() == 1
            && matches!(&results[0], Value::Array(array) if !array.sequence)
        {
            return Ok(results.pop().expect("one result"));
        }

        let mut flattened = Vec::with_capacity(results.len());
        for value in results {
            match value {
                Value::Array(array) if !array.cons => flattened.extend(array.iter()),
                value => flattened.push(value),
            }
        }

        Ok(Value::sequence(flattened))
    }

    fn tuple_step(
        &self,
        step: &'a Node,
        tuples: Vec<Rc<Tuple<'a>>>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Vec<Rc<Tuple<'a>>>> {
        if let Kind::Sort(terms) = &step.kind {
            let sorted = self.sort(
                terms,
                tuples.into_iter().map(Value::Tuple).collect(),
                env,
                step.position,
            )?;
            let mut result: Vec<Rc<Tuple<'a>>> = sorted
                .items()
                .into_iter()
                .filter_map(|item| match item {
                    Value::Tuple(tuple) => Some(tuple),
                    _ => None,
                })
                .collect();
            if let Some(index) = &step.index {
                result = result
                    .into_iter()
                    .enumerate()
                    .map(|(position, tuple)| {
                        with_binding(&tuple, index, Value::Number(position as f64))
                    })
                    .collect();
            }
            return self.tuple_stages(step, result, env);
        }

        let mut result = Vec::new();
        for tuple in &tuples {
            let frame = tuple_frame(env, tuple);
            let value = self.evaluate(step, &tuple.context, &frame)?;

            for (position, item) in value.items().into_iter().enumerate() {
                let mut next = Tuple {
                    context: item.clone(),
                    bindings: tuple.bindings.clone(),
                };
                if let Some(focus) = &step.focus {
                    next.bindings.push((focus.clone(), item));
                    next.context = tuple.context.clone();
                }
                if let Some(index) = &step.index {
                    next.bindings
                        .push((index.clone(), Value::Number(position as f64)));
                }
                result.push(Rc::new(next));
            }
        }

        self.tuple_stages(step, result, env)
    }

    fn tuple_stages(
        &self,
        step: &'a Node,
        mut tuples: Vec<Rc<Tuple<'a>>>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Vec<Rc<Tuple<'a>>>> {
        for stage in &step.stages {
            tuples = match &stage.kind {
                Kind::IndexBind(name) => tuples
                    .iter()
                    .enumerate()
                    .map(|(position, tuple)| {
                        with_binding(tuple, name, Value::Number(position as f64))
                    })
                    .collect(),
                _ => {
                    let filtered = self.filter(stage, tuple_sequence(&tuples), env)?;
                    filtered
                        .items()
                        .into_iter()
                        .filter_map(|item| match item {
                            Value::Tuple(tuple) => Some(tuple),
                            _ => None,
                        })
                        .collect()
                }
            };
        }

        Ok(tuples)
    }

    /// `items[predicate]`: the items at numeric indexes, or those for which
    /// the predicate is true.
    fn filter(
        &self,
        predicate: &'a Node,
        input: Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        let tuple_stream = matches!(&input, Value::Array(array) if array.tuple_stream);
        let items = input.items();

        if let Kind::Number(number) = predicate.kind
            && predicate.is_plain()
        {
            let index = resolve_index(number, items.len());
            return Ok(match index.and_then(|index| items.get(index)) {
                Some(Value::Array(array)) => Value::Array(array.clone()),
                Some(item) => Value::sequence(vec![item.clone()]),
                None => Value::sequence(Vec::new()),
            });
        }

        let mut results = Vec::new();
        for (position, item) in items.iter().enumerate() {
            let value = match item {
                Value::Tuple(tuple) => {
                    let frame = tuple_frame(env, tuple);
                    self.evaluate(predicate, &tuple.context, &frame)?
                }
                item => self.evaluate(predicate, item, env)?,
            };

            let indexes: Option<Vec<f64>> = match &value {
                Value::Number(number) => Some(vec![*number]),
                Value::Array(array)
                    if !array.is_empty()
                        && array.iter().all(|item| matches!(item, Value::Number(_))) =>
                {
                    Some(array.iter().filter_map(|item| item.as_number()).collect())
                }
                _ => None,
            };
            match indexes {
                Some(indexes) => {
                    if indexes
                        .iter()
                        .any(|&index| resolve_index(index, items.len()) == Some(position))
                    {
                        results.push(item.clone());
                    }
                }
                None => {
                    if value.truthy() == Some(true) {
                        results.push(item.clone());
                    }
                }
            }
        }

        let mut result = Value::sequence(results);
        if tuple_stream && let Value::Array(array) = &mut result {
            array.tuple_stream = true;
        }
        Ok(result)
    }

    fn range(
        &self,
        start: &'a Node,
        end: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Vec<Value<'a>>> {
        let from = self.evaluate(start, input, env)?;
        let to = self.evaluate(end, input, env)?;

        let integer = |value: &Value, code: &'static str, node: &Node| match value {
            Value::Undefined => Ok(None),
            Value::Number(number) if number.fract() == 0. => Ok(Some(*number as i64)),
            _ => Err(Error::at(
                code,
                node.position,
                "The ends of a range must be integers",
            )),
        };
        let (Some(from), Some(to)) = (integer(&from, "T2003", start)?, integer(&to, "T2004", end)?)
        else {
            return Ok(Vec::new());
        };
        if from > to {
            return Ok(Vec::new());
        }
        if i128::from(to) - i128::from(from) >= 10_000_000 {
            return Err(Error::at(
                "D2014",
                start.position,
                "The size of the sequence allocated by the range operator exceeds 10,000,000",
            ));
        }

        Ok((from..=to)
            .map(|number| Value::Number(number as f64))
            .collect())
    }

    fn group(&self, group: &'a Group, input: Value<'a>, env: &Rc<Frame<'a>>) -> Result<Value<'a>> {
        if let Value::Array(array) = &input
            && array.tuple_stream
        {
            let tuples = array
                .iter()
                .filter_map(|item| match item {
                    Value::Tuple(tuple) => Some(tuple),
                    _ => None,
                })
                .collect();
            return self.group_tuples(group, tuples, env);
        }

        self.group_pairs(&group.pairs, input, env)
    }

    fn group_pairs(
        &self,
        pairs: &'a [(Node, Node)],
        input: Value<'a>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        let mut items = input.items();
        // An empty input still builds a literal object.
        if items.is_empty() {
            items.push(Value::Undefined);
        }

        let mut groups: IndexMap<String, (Vec<Value<'a>>, usize)> = IndexMap::new();
        for item in items {
            for (index, (key, _)) in pairs.iter().enumerate() {
                let key_value = self.evaluate(key, &item, env)?;
                let key_text = match &key_value {
                    Value::Undefined => continue,
                    Value::String(text) => text.as_str().to_owned(),
                    _ => {
                        return Err(Error::at(
                            "T1003",
                            key.position,
                            format!(
                                "Key in object structure must evaluate to a string; got: {}",
                                describe(&key_value)
                            ),
                        ));
                    }
                };

                match groups.get_mut(&key_text) {
                    Some((data, pair)) => {
                        if *pair != index {
                            return Err(Error::at(
                                "D1009",
                                key.position,
                                format!(
                                    "Multiple key definitions evaluate to same key: \"{key_text}\""
                                ),
                            ));
                        }
                        data.push(item.clone());
                    }
                    None => {
                        groups.insert(key_text, (vec![item.clone()], index));
                    }
                }
            }
        }

        let mut object = IndexMap::with_capacity(groups.len());
        for (key, (data, index)) in groups {
            let context = if data.len() == 1 {
                data.into_iter().next().expect("one item")
            } else {
                Value::sequence(data.into_iter().flat_map(|item| item.items()).collect())
            };
            let value = self.evaluate(&pairs[index].1, &context, env)?;
            if !value.is_undefined() {
                object.insert(key, value);
            }
        }

        Ok(Value::object(object))
    }

    fn group_tuples(
        &self,
        group: &'a Group,
        tuples: Vec<Rc<Tuple<'a>>>,
        env: &Rc<Frame<'a>>,
    ) -> Result<Value<'a>> {
        let mut groups: IndexMap<String, (Vec<Rc<Tuple<'a>>>, usize)> = IndexMap::new();
        for tuple in &tuples {
            let frame = tuple_frame(env, tuple);
            for (index, (key, _)) in group.pairs.iter().enumerate() {
                let key_value = self.evaluate(key, &tuple.context, &frame)?;
                let key_text = match &key_value {
                    Value::Undefined => continue,
                    Value::String(text) => text.as_str().to_owned(),
                    _ => {
                        return Err(Error::at(
                            "T1003",
                            key.position,
                            "Key in object structure must evaluate to a string",
                        ));
                    }
                };
                match groups.get_mut(&key_text) {
                    Some((data, pair)) => {
                        if *pair != index {
                            return Err(Error::at(
                                "D1009",
                                key.position,
                                format!(
                                    "Multiple key definitions evaluate to same key: \"{key_text}\""
                                ),
                            ));
                        }
                        data.push(tuple.clone());
                    }
                    None => {
                        groups.insert(key_text, (vec![tuple.clone()], index));
                    }
                }
            }
        }

        let mut object = IndexMap::new();
        for (key, (data, index)) in groups {
            // The group's variables hold every value they took in it.
            let frame = Frame::child(env);
            let mut names: Vec<&str> = Vec::new();
            for tuple in &data {
                for (name, _) in &tuple.bindings {
                    if !names.contains(&name.as_str()) {
                        names.push(name);
                    }
                }
            }
            for name in names {
                let values: Vec<Value<'a>> = data
                    .iter()
                    .filter_map(|tuple| {
                        tuple.bindings.iter().rev().find(|(bound, _)| bound == name)
                    })
                    .map(|(_, value)| value.clone())
                    .collect();
                frame.bind(
                    name,
                    if values.len() == 1 {
                        values[0].clone()
                    } else {
                        Value::sequence(values)
                    },
                );
            }
            let context = if data.len() == 1 {
                data[0].context.clone()
            } else {
                Value::sequence(data.iter().map(|tuple| tuple.context.clone()).collect())
            };
            let value = self.evaluate(&group.pairs[index].1, &context, &frame)?;
            if !value.is_undefined() {
                object.insert(key, value);
            }
        }

        Ok(Value::object(object))
    }

    fn binary(
        &self,
        operator: Operator,
        lhs: &'a Node,
        rhs: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
        position: usize,
    ) -> Result<Value<'a>> {
        let left = self.evaluate(lhs, input, env)?;

        match operator {
            Operator::And => {
                return Ok(Value::Bool(
                    left.truthy() == Some(true)
                        && self.evaluate(rhs, input, env)?.truthy() == Some(true),
                ));
            }
            Operator::Or => {
                return Ok(Value::Bool(
                    left.truthy() == Some(true)
                        || self.evaluate(rhs, input, env)?.truthy() == Some(true),
                ));
            }
            Operator::Coalesce => {
                return if left.is_undefined() {
                    self.evaluate(rhs, input, env)
                } else {
                    Ok(left)
                };
            }
            Operator::Default => {
                return if left.truthy() == Some(true) {
                    Ok(left)
                } else {
                    self.evaluate(rhs, input, env)
                };
            }
            _ => {}
        }

        let right = self.evaluate(rhs, input, env)?;
        match operator {
            Operator::Add
            | Operator::Subtract
            | Operator::Multiply
            | Operator::Divide
            | Operator::Remainder => {
                let number = |value: &Value, code: &'static str| match value {
                    Value::Undefined => Ok(None),
                    Value::Number(number) => Ok(Some(*number)),
                    value => Err(Error::at(
                        code,
                        position,
                        format!(
                            "The {} side of the \"{}\" operator must evaluate to a number; got {}",
                            if code == "T2001" { "left" } else { "right" },
                            symbol(operator),
                            describe(value)
                        ),
                    )),
                };
                let (Some(a), Some(b)) = (number(&left, "T2001")?, number(&right, "T2002")?) else {
                    return Ok(Value::Undefined);
                };
                let result = match operator {
                    Operator::Add => a + b,
                    Operator::Subtract => a - b,
                    Operator::Multiply => a * b,
                    Operator::Divide => a / b,
                    _ => a % b,
                };
                if !result.is_finite() {
                    return Err(Error::at(
                        "D1001",
                        position,
                        format!("Number out of range: {}", format_number(result)),
                    ));
                }
                Ok(Value::Number(result))
            }
            Operator::Equal => Ok(Value::Bool(
                !left.is_undefined() && !right.is_undefined() && equal(&left, &right),
            )),
            Operator::NotEqual => Ok(Value::Bool(
                !left.is_undefined() && !right.is_undefined() && !equal(&left, &right),
            )),
            Operator::Less
            | Operator::LessOrEqual
            | Operator::Greater
            | Operator::GreaterOrEqual => {
                let comparable = |value: &Value| {
                    matches!(
                        value,
                        Value::Undefined | Value::Number(_) | Value::String(_)
                    )
                };
                if !comparable(&left) || !comparable(&right) {
                    let value = if comparable(&left) { &right } else { &left };
                    return Err(Error::at(
                        "T2010",
                        position,
                        format!(
                            "The expressions on either side of operator \"{}\" must evaluate to numeric or string values; got {}",
                            symbol(operator),
                            describe(value)
                        ),
                    ));
                }
                let ordering = match (&left, &right) {
                    (Value::Undefined, _) | (_, Value::Undefined) => return Ok(Value::Undefined),
                    (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                    (Value::String(a), Value::String(b)) => Some(a.as_str().cmp(b.as_str())),
                    _ => {
                        return Err(Error::at(
                            "T2009",
                            position,
                            format!(
                                "The values {} and {} either side of operator \"{}\" must be of the same data type",
                                describe(&left),
                                describe(&right),
                                symbol(operator)
                            ),
                        ));
                    }
                };
                let Some(ordering) = ordering else {
                    return Ok(Value::Bool(false));
                };
                Ok(Value::Bool(match operator {
                    Operator::Less => ordering.is_lt(),
                    Operator::LessOrEqual => ordering.is_le(),
                    Operator::Greater => ordering.is_gt(),
                    _ => ordering.is_ge(),
                }))
            }
            Operator::Concat => {
                let text = |value: &Value<'a>| -> Result<String> {
                    Ok(match value {
                        Value::Undefined => String::new(),
                        value => functions::stringify(value, false)
                            .map_err(|error| error.or_at(position))?,
                    })
                };
                Ok(Value::string(text(&left)? + &text(&right)?))
            }
            Operator::In => {
                if left.is_undefined() || right.is_undefined() {
                    return Ok(Value::Bool(false));
                }
                // Membership compares like JavaScript's ===: objects and
                // arrays are never members.
                let scalar = |a: &Value<'a>, b: &Value<'a>| match (a, b) {
                    (Value::Array(_) | Value::Object(_), _)
                    | (_, Value::Array(_) | Value::Object(_)) => false,
                    (a, b) => equal(a, b),
                };
                Ok(Value::Bool(
                    right.items().iter().any(|item| scalar(item, &left)),
                ))
            }
            _ => unreachable!("handled above"),
        }
    }

    fn call(
        &self,
        procedure: &'a Node,
        arguments: &'a [Node],
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
        first: Option<Value<'a>>,
        position: usize,
    ) -> Result<Value<'a>> {
        let function = self.evaluate(procedure, input, env)?;
        self.call_with(function, procedure, arguments, input, env, first, position)
    }

    #[allow(clippy::too_many_arguments)]
    fn call_with(
        &self,
        function: Value<'a>,
        procedure: &'a Node,
        arguments: &'a [Node],
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
        first: Option<Value<'a>>,
        position: usize,
    ) -> Result<Value<'a>> {
        if function.is_undefined()
            && let Kind::Path { steps, .. } = &procedure.kind
            && let [
                Node {
                    kind: Kind::Name(name),
                    ..
                },
            ] = steps.as_slice()
            && (functions::find(name).is_some() || env.lookup(name).is_some())
        {
            return Err(Error::at(
                "T1005",
                position,
                format!("Attempted to invoke a non-function. Did you mean ${name}?"),
            ));
        }

        let partial = arguments
            .iter()
            .any(|argument| matches!(argument.kind, Kind::Placeholder));
        let mut values: Vec<Option<Value<'a>>> = first.map(Some).into_iter().collect();
        for argument in arguments {
            values.push(match argument.kind {
                Kind::Placeholder => None,
                _ => Some(self.evaluate(argument, input, env)?),
            });
        }

        if partial {
            if !function.is_function() {
                return Err(Error::at(
                    "T1008",
                    position,
                    "Attempted to partially apply a non-function",
                ));
            }
            return Ok(Value::function(Function::Partial {
                function,
                arguments: values,
            }));
        }

        let values = values
            .into_iter()
            .map(|value| value.unwrap_or(Value::Undefined))
            .collect();
        self.apply_in(env, &function, values, input, position)
    }

    /// `lhs ~> rhs`: call `rhs` with `lhs` first, or chain two functions.
    fn apply_operator(
        &self,
        lhs: &'a Node,
        rhs: &'a Node,
        input: &Value<'a>,
        env: &Rc<Frame<'a>>,
        position: usize,
    ) -> Result<Value<'a>> {
        let left = self.evaluate(lhs, input, env)?;

        if let Kind::Call {
            procedure,
            arguments,
        } = &rhs.kind
            && rhs.is_plain()
        {
            return self.call(procedure, arguments, input, env, Some(left), rhs.position);
        }

        let function = self.evaluate(rhs, input, env)?;
        if !function.is_function() {
            return Err(Error::at(
                "T2006",
                position,
                "The right side of the function application operator ~> must be a function",
            ));
        }

        if left.is_function() {
            Ok(Value::function(Function::Chain(left, function)))
        } else {
            self.apply_in(env, &function, vec![left], input, position)
        }
    }

    /// Apply a function called in the scope `env`.
    fn apply_in(
        &self,
        env: &Rc<Frame<'a>>,
        function: &Value<'a>,
        arguments: Vec<Value<'a>>,
        input: &Value<'a>,
        position: usize,
    ) -> Result<Value<'a>> {
        self.scopes.borrow_mut().push(env.clone());
        let result = self.apply(function, arguments, input, position);
        self.scopes.borrow_mut().pop();
        result
    }

    pub fn apply(
        &self,
        function: &Value<'a>,
        arguments: Vec<Value<'a>>,
        input: &Value<'a>,
        position: usize,
    ) -> Result<Value<'a>> {
        let mut outcome = self.apply_once(function, arguments, input, position)?;
        loop {
            match outcome {
                Outcome::Value(value) => return Ok(value),
                Outcome::TailCall(function, arguments) => {
                    outcome = self.apply_once(&function, arguments, input, position)?;
                }
            }
        }
    }

    fn apply_once(
        &self,
        function: &Value<'a>,
        arguments: Vec<Value<'a>>,
        input: &Value<'a>,
        position: usize,
    ) -> Result<Outcome<'a>> {
        let Value::Function(function) = function else {
            if let Value::Regex(regex) = function {
                return functions::regex_match(regex, arguments.first(), position)
                    .map(Outcome::Value);
            }
            return Err(Error::at(
                "T1006",
                position,
                "Attempted to invoke a non-function",
            ));
        };

        match &**function {
            Function::Lambda {
                definition,
                environment,
                input: captured,
            } => self.lambda(definition, environment, captured, arguments, position),
            Function::Builtin(builtin) => {
                functions::call(self, builtin, arguments, input, position).map(Outcome::Value)
            }
            Function::Partial {
                function,
                arguments: bound,
            } => {
                let mut given = arguments.into_iter();
                let filled = bound
                    .iter()
                    .map(|argument| match argument {
                        Some(value) => value.clone(),
                        None => given.next().unwrap_or(Value::Undefined),
                    })
                    .collect();
                self.apply_once(function, filled, input, position)
            }
            Function::Chain(first, second) => {
                let value = self.apply(first, arguments, input, position)?;
                self.apply(second, vec![value], input, position)
                    .map(Outcome::Value)
            }
            Function::Transform { node, environment } => {
                let argument = arguments.into_iter().next().unwrap_or(Value::Undefined);
                self.transform(node, environment, argument)
                    .map(Outcome::Value)
            }
        }
    }

    fn lambda(
        &self,
        definition: &'a Lambda,
        environment: &Rc<Frame<'a>>,
        input: &Value<'a>,
        arguments: Vec<Value<'a>>,
        position: usize,
    ) -> Result<Outcome<'a>> {
        let frame = Frame::child(environment);
        let mut arguments = arguments.into_iter();
        for parameter in &definition.parameters {
            frame.bind(parameter, arguments.next().unwrap_or(Value::Undefined));
        }

        self.enter(position)?;
        let outcome = self.evaluate_tail(&definition.body, input, &frame);
        self.leave();
        self.close(frame);
        outcome
    }

    /// How many arguments a function takes, which decides what higher-order
    /// functions pass it.
    pub fn arity(&self, function: &Value) -> usize {
        match function {
            Value::Function(function) => match &**function {
                Function::Lambda { definition, .. } => definition.parameters.len(),
                // As JSONata counts a native function's parameters, leaving
                // out those with defaults.
                Function::Builtin(builtin) => builtin.min,
                Function::Partial { arguments, .. } => arguments
                    .iter()
                    .filter(|argument| argument.is_none())
                    .count(),
                Function::Chain(first, _) => self.arity(first),
                Function::Transform { .. } => 1,
            },
            Value::Regex(_) => 1,
            _ => 0,
        }
    }

    pub fn sort(
        &self,
        terms: &'a [(Node, bool)],
        items: Vec<Value<'a>>,
        env: &Rc<Frame<'a>>,
        position: usize,
    ) -> Result<Value<'a>> {
        let tuple_stream = items.iter().any(|item| matches!(item, Value::Tuple(_)));
        let mut keyed = Vec::with_capacity(items.len());
        for item in items {
            let mut keys = Vec::with_capacity(terms.len());
            for (term, _) in terms {
                let key = match &item {
                    Value::Tuple(tuple) => {
                        self.evaluate(term, &tuple.context, &tuple_frame(env, tuple))?
                    }
                    item => self.evaluate(term, item, env)?,
                };
                if !matches!(key, Value::Undefined | Value::Number(_) | Value::String(_)) {
                    return Err(Error::at(
                        "T2008",
                        position,
                        "The expressions within an order-by clause must evaluate to numeric or string values",
                    ));
                }
                keys.push(key);
            }
            keyed.push((keys, item));
        }

        let mut error = None;
        // Vec::sort_by is stable, as JSONata's merge sort is.
        keyed.sort_by(|(a, _), (b, _)| {
            for (index, (_, descending)) in terms.iter().enumerate() {
                let ordering = match (&a[index], &b[index]) {
                    (Value::Undefined, Value::Undefined) => std::cmp::Ordering::Equal,
                    (Value::Undefined, _) => return std::cmp::Ordering::Greater,
                    (_, Value::Undefined) => return std::cmp::Ordering::Less,
                    (Value::Number(a), Value::Number(b)) => {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    (Value::String(a), Value::String(b)) => a.as_str().cmp(b.as_str()),
                    _ => {
                        error.get_or_insert_with(|| {
                            Error::at(
                                "T2007",
                                position,
                                "Type mismatch when comparing values in order-by clause",
                            )
                        });
                        std::cmp::Ordering::Equal
                    }
                };
                let ordering = if *descending {
                    ordering.reverse()
                } else {
                    ordering
                };
                if ordering.is_ne() {
                    return ordering;
                }
            }
            std::cmp::Ordering::Equal
        });
        if let Some(error) = error {
            return Err(error);
        }

        let mut result = Value::sequence(keyed.into_iter().map(|(_, item)| item).collect());
        if tuple_stream && let Value::Array(array) = &mut result {
            array.tuple_stream = true;
        }
        Ok(result)
    }

    /// `| pattern | update, delete |` on a copy of `value`: each object the
    /// pattern matches gets the update's fields and loses the deleted ones.
    fn transform(
        &self,
        node: &'a Node,
        env: &Rc<Frame<'a>>,
        value: Value<'a>,
    ) -> Result<Value<'a>> {
        let Kind::Transform {
            pattern,
            update,
            delete,
        } = &node.kind
        else {
            return Ok(Value::Undefined);
        };
        if value.is_undefined() {
            return Ok(Value::Undefined);
        }

        // A deep copy gives every object its own identity to match.
        let copy = deep_copy(&value);
        let matches = self.evaluate(pattern, &copy, env)?;
        let mut changes: Changes<'a> = HashMap::new();

        for matched in matches.items() {
            let Value::Object(Object::Owned(object)) = &matched else {
                continue;
            };
            let fields = match self.evaluate(update, &matched, env)? {
                Value::Undefined => IndexMap::new(),
                Value::Object(update) => update.to_map(),
                other => {
                    return Err(Error::at(
                        "T2011",
                        update.position,
                        format!(
                            "The insert/update clause of the transform expression must evaluate to an object: {}",
                            describe(&other)
                        ),
                    ));
                }
            };
            let mut deletions = Vec::new();
            if let Some(delete) = delete {
                match self.evaluate(delete, &matched, env)? {
                    Value::Undefined => {}
                    value => {
                        for item in value.items() {
                            match item.as_str() {
                                Some(name) => deletions.push(name.to_owned()),
                                None => {
                                    return Err(Error::at(
                                        "T2012",
                                        delete.position,
                                        "The delete clause of the transform expression must evaluate to a string or array of strings",
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            changes
                .entry(Rc::as_ptr(object))
                .or_default()
                .push((fields, deletions));
        }

        Ok(rebuild(&copy, &changes))
    }
}

/// The fields to update and delete in each matched object, by its identity.
type Changes<'a> =
    HashMap<*const IndexMap<String, Value<'a>>, Vec<(IndexMap<String, Value<'a>>, Vec<String>)>>;

/// The copy with each matched object changed, children first.
fn rebuild<'a>(value: &Value<'a>, changes: &Changes<'a>) -> Value<'a> {
    match value {
        Value::Array(array) => {
            let mut rebuilt = array.clone();
            rebuilt.items = Items::Owned(Rc::new(
                array.iter().map(|item| rebuild(&item, changes)).collect(),
            ));
            Value::Array(rebuilt)
        }
        Value::Object(Object::Owned(object)) => {
            let mut fields: IndexMap<String, Value<'a>> = object
                .iter()
                .map(|(key, value)| (key.clone(), rebuild(value, changes)))
                .collect();
            for (update, deletions) in changes.get(&Rc::as_ptr(object)).into_iter().flatten() {
                for (key, value) in update {
                    fields.insert(key.clone(), value.clone());
                }
                for name in deletions {
                    fields.shift_remove(name);
                }
            }
            Value::object(fields)
        }
        value => value.clone(),
    }
}

/// A copy whose arrays and objects are all owned, and so distinct.
fn deep_copy<'a>(value: &Value<'a>) -> Value<'a> {
    match value {
        Value::Array(array) => {
            let mut copy = array.clone();
            copy.items = Items::Owned(Rc::new(array.iter().map(|item| deep_copy(&item)).collect()));
            Value::Array(copy)
        }
        Value::Object(object) => Value::object(
            object
                .entries()
                .into_iter()
                .map(|(key, value)| (key, deep_copy(&value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

/// A field of an object, or of each object in an array, flattened.
pub(crate) fn lookup<'a>(input: &Value<'a>, key: &str) -> Value<'a> {
    match input {
        Value::Array(array) => {
            let mut results = Vec::new();
            for item in array.iter() {
                match lookup(&item, key) {
                    Value::Undefined => {}
                    Value::Array(found) => results.extend(found.iter()),
                    found => results.push(found),
                }
            }
            Value::sequence(results)
        }
        Value::Object(object) => object.get(key).unwrap_or(Value::Undefined),
        Value::Tuple(tuple) => lookup(&tuple.context, key),
        _ => Value::Undefined,
    }
}

fn wildcard<'a>(input: &Value<'a>) -> Value<'a> {
    let input = match input {
        Value::Array(array) if array.outer_wrapper && !array.is_empty() => {
            array.get(0).unwrap_or(Value::Undefined)
        }
        input => input.clone(),
    };

    let mut results = Vec::new();
    if let Value::Object(object) = &input {
        for (_, value) in object.entries() {
            match value {
                Value::Array(_) => flatten(&value, &mut results),
                value => results.push(value),
            }
        }
    }
    Value::sequence(results)
}

fn flatten<'a>(value: &Value<'a>, results: &mut Vec<Value<'a>>) {
    match value {
        Value::Array(array) => {
            for item in array.iter() {
                flatten(&item, results);
            }
        }
        value => results.push(value.clone()),
    }
}

fn descendants<'a>(input: &Value<'a>) -> Value<'a> {
    fn visit<'a>(value: &Value<'a>, results: &mut Vec<Value<'a>>) {
        match value {
            Value::Array(array) => {
                for item in array.iter() {
                    visit(&item, results);
                }
            }
            Value::Object(object) => {
                results.push(value.clone());
                for (_, child) in object.entries() {
                    visit(&child, results);
                }
            }
            value => results.push(value.clone()),
        }
    }

    if input.is_undefined() {
        return Value::Undefined;
    }
    let mut results = Vec::new();
    visit(input, &mut results);
    if results.len() == 1 {
        results.pop().expect("one result")
    } else {
        Value::sequence(results)
    }
}

/// An index into a list of `length` items; negative ones count from the end.
fn resolve_index(index: f64, length: usize) -> Option<usize> {
    let index = index.floor();
    let index = if index < 0. {
        length as f64 + index
    } else {
        index
    };
    (index >= 0. && index < length as f64).then_some(index as usize)
}

fn tuple_frame<'a>(env: &Rc<Frame<'a>>, tuple: &Tuple<'a>) -> Rc<Frame<'a>> {
    let frame = Frame::child(env);
    for (name, value) in &tuple.bindings {
        frame.bind(name, value.clone());
    }
    frame
}

fn tuple_sequence<'a>(tuples: &[Rc<Tuple<'a>>]) -> Value<'a> {
    let mut sequence = Value::sequence(tuples.iter().cloned().map(Value::Tuple).collect());
    if let Value::Array(array) = &mut sequence {
        array.tuple_stream = true;
    }
    sequence
}

fn with_binding<'a>(tuple: &Tuple<'a>, name: &str, value: Value<'a>) -> Rc<Tuple<'a>> {
    let mut bindings = tuple.bindings.clone();
    bindings.push((name.to_owned(), value));
    Rc::new(Tuple {
        context: tuple.context.clone(),
        bindings,
    })
}

fn symbol(operator: Operator) -> &'static str {
    match operator {
        Operator::Add => "+",
        Operator::Subtract => "-",
        Operator::Multiply => "*",
        Operator::Divide => "/",
        Operator::Remainder => "%",
        Operator::Equal => "=",
        Operator::NotEqual => "!=",
        Operator::Less => "<",
        Operator::LessOrEqual => "<=",
        Operator::Greater => ">",
        Operator::GreaterOrEqual => ">=",
        Operator::Concat => "&",
        Operator::And => "and",
        Operator::Or => "or",
        Operator::In => "in",
        Operator::Coalesce => "??",
        Operator::Default => "?:",
    }
}

/// A value as errors quote it.
pub(crate) fn describe(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".to_owned(),
        Value::Function(_) | Value::Regex(_) => "a function".to_owned(),
        value => {
            let text = functions::stringify(value, false).unwrap_or_default();
            let text = match value {
                Value::String(_) => format!("\"{text}\""),
                _ => text,
            };
            if text.chars().count() > 60 {
                format!("{}…", text.chars().take(60).collect::<String>())
            } else {
                text
            }
        }
    }
}
