use indexmap::IndexMap;

use super::registry::{Args, Builtin};
use super::strings::{builtin, owned};
use crate::error::Result;
use crate::evaluator::lookup;
use crate::value::Value;

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "keys",
        "$keys(object)",
        "The keys of an object, or of every object in an array",
        1,
        1,
        true,
        keys,
    ),
    builtin(
        "lookup",
        "$lookup(object, key)",
        "The value of a key, from an object or each object in an array",
        2,
        2,
        true,
        lookup_,
    ),
    builtin(
        "spread",
        "$spread(object)",
        "An array of objects with one key each",
        1,
        1,
        true,
        spread,
    ),
    builtin(
        "merge",
        "$merge(array)",
        "Merges an array of objects into one; later keys win",
        1,
        1,
        false,
        merge,
    ),
    builtin(
        "sift",
        "$sift(object, function)",
        "The keys of an object for which a function of value and key is true",
        1,
        2,
        true,
        sift,
    ),
    builtin(
        "each",
        "$each(object, function)",
        "The results of a function for each value and key of an object",
        2,
        2,
        true,
        each,
    ),
    builtin(
        "error",
        "$error([message])",
        "Stops evaluation with an error",
        0,
        1,
        false,
        error,
    ),
    builtin(
        "assert",
        "$assert(condition[, message])",
        "Stops evaluation with an error unless the condition is true",
        1,
        2,
        false,
        assert,
    ),
    builtin(
        "type",
        "$type(value)",
        "The type of a value: null, number, string, boolean, array, object or function",
        1,
        1,
        false,
        type_,
    ),
    builtin(
        "clone",
        "$clone(value)",
        "A deep copy of a value",
        1,
        1,
        true,
        clone,
    ),
];

fn keys<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let mut keys: Vec<String> = Vec::new();
    let mut add = |object: &crate::value::Object| {
        for key in object.keys() {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    };

    match args.get(0) {
        Value::Object(object) => add(&object),
        Value::Array(array) => {
            for item in array.iter() {
                if let Value::Object(object) = item {
                    add(&object);
                }
            }
        }
        _ => {}
    }

    Ok(Value::sequence(
        keys.into_iter().map(Value::string).collect(),
    ))
}

fn lookup_<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let key = args.string(1)?.ok_or_else(|| args.mismatch(1))?;
    Ok(lookup(&args.get(0), key))
}

fn spread<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    fn single<'a>(key: String, value: Value<'a>) -> Value<'a> {
        let mut object = IndexMap::new();
        object.insert(key, value);
        Value::object(object)
    }

    Ok(match args.get(0) {
        Value::Object(object) => Value::sequence(
            object
                .entries()
                .into_iter()
                .map(|(key, value)| single(key, value))
                .collect(),
        ),
        Value::Array(array) => {
            let mut results = Vec::new();
            for item in array.iter() {
                match item {
                    Value::Object(object) => results.extend(
                        object
                            .entries()
                            .into_iter()
                            .map(|(key, value)| single(key, value)),
                    ),
                    item => results.push(item),
                }
            }
            Value::sequence(results)
        }
        value => value,
    })
}

fn merge<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };

    let mut merged = IndexMap::new();
    for item in items {
        match item {
            Value::Object(object) => merged.extend(object.entries()),
            _ => {
                return Err(args.error(
                    "T0412",
                    "Argument 1 of function \"merge\" must be an array of objects",
                ));
            }
        }
    }

    Ok(Value::object(merged))
}

fn sift<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(object) = args.object(0)? else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?.ok_or_else(|| args.mismatch(1))?;
    let whole = Value::Object(object.clone());

    let mut kept = IndexMap::new();
    for (key, value) in object.entries() {
        let keep = args.apply_some(
            &function,
            vec![value.clone(), Value::string(key.as_str()), whole.clone()],
        )?;
        if keep.truthy() == Some(true) {
            kept.insert(key, value);
        }
    }

    Ok(if kept.is_empty() {
        Value::Undefined
    } else {
        Value::object(kept)
    })
}

fn each<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(object) = args.object(0)? else {
        return Ok(Value::Undefined);
    };
    let function = args.function(1)?.ok_or_else(|| args.mismatch(1))?;
    let whole = Value::Object(object.clone());

    let mut results = Vec::new();
    for (key, value) in object.entries() {
        let result = args.apply_some(
            &function,
            vec![value, Value::string(key.as_str()), whole.clone()],
        )?;
        if !result.is_undefined() {
            results.push(result);
        }
    }

    Ok(Value::sequence(results))
}

fn error<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let message = args.string(0)?.unwrap_or("$error() function evaluated");
    Err(args.error("D3137", message))
}

fn assert<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let condition = args.boolean(0)?.ok_or_else(|| args.mismatch(0))?;
    if condition {
        return Ok(Value::Undefined);
    }

    let message = args.string(1)?.unwrap_or("$assert() statement failed");
    Err(args.error("D3141", message))
}

fn type_<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.get(0) {
        Value::Undefined => Value::Undefined,
        value => Value::string(value.type_name()),
    })
}

fn clone<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.get(0).to_json() {
        Some(json) => owned(&json),
        None => Value::Undefined,
    })
}
