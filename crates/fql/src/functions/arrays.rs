use super::registry::{Args, Builtin};
use super::strings::builtin;
use crate::error::Result;
use crate::value::{Value, equal};

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "append",
        "$append(array1, array2)",
        "Joins two arrays; single values count as arrays of one",
        2,
        2,
        false,
        append,
    ),
    builtin(
        "sort",
        "$sort(array[, function])",
        "Sorts numbers or strings, or by a function that returns true to swap two items",
        1,
        2,
        false,
        sort,
    ),
    builtin(
        "reverse",
        "$reverse(array)",
        "The array in reverse order",
        1,
        1,
        false,
        reverse,
    ),
    builtin(
        "shuffle",
        "$shuffle(array)",
        "The array in random order",
        1,
        1,
        false,
        shuffle,
    ),
    builtin(
        "distinct",
        "$distinct(array)",
        "The array without duplicates",
        1,
        1,
        false,
        distinct,
    ),
    builtin(
        "zip",
        "$zip(array1, …)",
        "Arrays of the items at each position of the arrays",
        1,
        usize::MAX,
        false,
        zip,
    ),
    builtin(
        "map",
        "$map(array, function)",
        "The results of a function for each item, called with the item, index and array",
        2,
        2,
        false,
        map,
    ),
    builtin(
        "filter",
        "$filter(array, function)",
        "The items for which a function is true",
        2,
        2,
        false,
        filter,
    ),
    builtin(
        "single",
        "$single(array[, function])",
        "The only item for which a function is true; fails unless there is exactly one",
        1,
        2,
        false,
        single,
    ),
    builtin(
        "reduce",
        "$reduce(array, function[, init])",
        "Folds an array: the function gets the result so far and each item",
        2,
        3,
        false,
        reduce,
    ),
];

fn append<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    match (args.get(0), args.get(1)) {
        (Value::Undefined, second) => Ok(second),
        (first, Value::Undefined) => Ok(first),
        (first, second) => {
            let mut items = first.items();
            items.extend(second.items());
            Ok(Value::array(items))
        }
    }
}

fn sort<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let value = args.get(0);
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    if items.len() <= 1 {
        return Ok(value);
    }

    let comparator = args.function(1)?;
    let sorted = match comparator {
        Some(comparator) => merge_sort(items, &mut |a, b| {
            Ok(args
                .apply(&comparator, vec![a.clone(), b.clone()])?
                .truthy()
                == Some(true))
        })?,
        None => {
            let numbers = items.iter().all(|item| matches!(item, Value::Number(_)));
            let strings = items.iter().all(|item| matches!(item, Value::String(_)));
            if !numbers && !strings {
                return Err(args.error("D3070", "The single argument form of the sort function can only be applied to an array of strings or an array of numbers. Use the second argument to specify a comparison function"));
            }
            merge_sort(items, &mut |a, b| {
                Ok(match (a, b) {
                    (Value::Number(a), Value::Number(b)) => a > b,
                    (Value::String(a), Value::String(b)) => a.as_str() > b.as_str(),
                    _ => false,
                })
            })?
        }
    };

    Ok(Value::array(sorted))
}

/// A stable merge sort. `swap(a, b)` tells whether `a` belongs after `b`.
fn merge_sort<'a>(
    items: Vec<Value<'a>>,
    swap: &mut dyn FnMut(&Value<'a>, &Value<'a>) -> Result<bool>,
) -> Result<Vec<Value<'a>>> {
    if items.len() <= 1 {
        return Ok(items);
    }

    let mut right = items;
    let left = right.drain(..right.len() / 2).collect();
    let left = merge_sort(left, swap)?;
    let right = merge_sort(right, swap)?;

    let mut merged = Vec::with_capacity(left.len() + right.len());
    let (mut left, mut right) = (left.into_iter().peekable(), right.into_iter().peekable());
    while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
        if swap(a, b)? {
            merged.push(right.next().expect("peeked"));
        } else {
            merged.push(left.next().expect("peeked"));
        }
    }
    merged.extend(left);
    merged.extend(right);

    Ok(merged)
}

fn reverse<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.array(0) {
        Some(mut items) => {
            items.reverse();
            Value::array(items)
        }
        None => Value::Undefined,
    })
}

fn shuffle<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(mut items) = args.array(0) else {
        return Ok(Value::Undefined);
    };

    for index in (1..items.len()).rev() {
        let other = (args.evaluation.random() * (index + 1) as f64) as usize;
        items.swap(index, other.min(index));
    }

    Ok(Value::array(items))
}

fn distinct<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let value = args.get(0);
    let Value::Array(array) = &value else {
        return Ok(value);
    };
    if array.len() <= 1 {
        return Ok(value);
    }

    let mut unique: Vec<Value<'a>> = Vec::new();
    for item in array.iter() {
        if !unique.iter().any(|existing| equal(existing, &item)) {
            unique.push(item);
        }
    }

    // A sequence stays a sequence; an array stays an array.
    Ok(if array.sequence {
        Value::sequence(unique)
    } else {
        Value::array(unique)
    })
}

fn zip<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let arrays: Vec<Vec<Value<'a>>> = (0..args.len())
        .map(|index| args.array(index).unwrap_or_default())
        .collect();
    let length = arrays.iter().map(Vec::len).min().unwrap_or(0);

    Ok(Value::array(
        (0..length)
            .map(|index| Value::array(arrays.iter().map(|array| array[index].clone()).collect()))
            .collect(),
    ))
}

fn map<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?.ok_or_else(|| args.mismatch(1))?;
    let array = Value::array(items.clone());

    let mut results = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let result = args.apply_some(
            &function,
            vec![item, Value::Number(index as f64), array.clone()],
        )?;
        if !result.is_undefined() {
            results.push(result);
        }
    }

    Ok(Value::sequence(results))
}

fn filter<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?.ok_or_else(|| args.mismatch(1))?;
    let array = Value::array(items.clone());

    let mut results = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let keep = args.apply_some(
            &function,
            vec![item.clone(), Value::Number(index as f64), array.clone()],
        )?;
        if keep.truthy() == Some(true) {
            results.push(item);
        }
    }

    Ok(Value::sequence(results))
}

fn single<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?;
    let array = Value::array(items.clone());

    let mut found = None;
    for (index, item) in items.into_iter().enumerate() {
        let matches = match &function {
            Some(function) => {
                args.apply_some(
                    function,
                    vec![item.clone(), Value::Number(index as f64), array.clone()],
                )?
                .truthy()
                    == Some(true)
            }
            None => true,
        };
        if matches {
            if found.is_some() {
                return Err(args.error("D3138", "The $single() function expected exactly 1 matching result. Instead it matched more."));
            }
            found = Some(item);
        }
    }

    found.ok_or_else(|| {
        args.error(
            "D3139",
            "The $single() function expected exactly 1 matching result. Instead it matched 0.",
        )
    })
}

fn reduce<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?.ok_or_else(|| args.mismatch(1))?;
    if args.evaluation.arity(&function) < 2 {
        return Err(args.error(
            "D3050",
            "The second argument of reduce function must be a function with at least two arguments",
        ));
    }
    let array = Value::array(items.clone());

    let mut items = items.into_iter().enumerate();
    let mut accumulator = match args.get(2) {
        Value::Undefined if args.len() < 3 => match items.next() {
            Some((_, first)) => first,
            None => return Ok(Value::Undefined),
        },
        init => init,
    };
    for (index, item) in items {
        accumulator = args.apply_some(
            &function,
            vec![
                accumulator,
                item,
                Value::Number(index as f64),
                array.clone(),
            ],
        )?;
    }

    Ok(accumulator)
}
