use std::collections::BTreeMap;

use serde_json::Value;

/// Freeze collection defaults into imported requests. Unknown names remain
/// placeholders for the user's selected environment at execution time.
pub(super) fn resolve_collection_defaults(collection: &mut Value) -> Result<(), String> {
    let Some(variables) = collection.get("variable").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut defaults = BTreeMap::new();

    // A later enabled definition replaces the whole earlier definition, including
    // its type. Do not validate a shadowed value that Postman would never use.
    for variable in variables.iter().rev() {
        if variable.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        let key = variable
            .get("key")
            .and_then(Value::as_str)
            .ok_or("A Postman collection variable has no key.")?;

        if defaults.contains_key(key) {
            continue;
        }

        let value = variable.get("value");
        let declared_type = match variable.get("type") {
            None | Some(Value::Null) => "any",
            Some(Value::String(name)) => name,
            _ => return Err(format!("Postman variable {key:?} has an unsupported type.")),
        };
        let supported = match declared_type.to_ascii_lowercase().as_str() {
            "any" => true,
            "string" => matches!(value, Some(Value::String(_))),
            "number" => matches!(value, Some(Value::Number(_))),
            "boolean" => matches!(value, Some(Value::Bool(_))),
            _ => false,
        };

        if !supported {
            return Err(format!(
                "Postman variable {key:?} uses unsupported type conversion. Use an untyped default or a string, number, or boolean matching its declared type before importing."
            ));
        }

        let value = match value {
            None => String::new(),
            Some(Value::Null) => "null".into(),
            Some(Value::String(value)) => value.clone(),
            Some(Value::Bool(value)) => value.to_string(),
            Some(Value::Number(value)) => value.to_string(),
            _ => {
                return Err(format!(
                    "Postman variable {key:?} must contain text, a number, or a boolean."
                ));
            }
        };
        defaults.insert(key.to_owned(), value);
    }

    replace_strings(
        collection,
        &defaults,
        &mut BTreeMap::new(),
        &mut (16 * 1024 * 1024),
    )
}

fn replace_strings(
    value: &mut Value,
    defaults: &BTreeMap<String, String>,
    cache: &mut BTreeMap<String, String>,
    remaining: &mut usize,
) -> Result<(), String> {
    match value {
        Value::String(text) => {
            let expanded = expand(text, defaults, cache, &mut Vec::new())?;
            *remaining = remaining
                .checked_sub(expanded.len())
                .ok_or("Expanded Postman values exceed the 16 MiB import limit.")?;
            *text = expanded;
        }
        Value::Array(values) => {
            for value in values {
                replace_strings(value, defaults, cache, remaining)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                replace_strings(value, defaults, cache, remaining)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn expand(
    input: &str,
    defaults: &BTreeMap<String, String>,
    cache: &mut BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, String> {
    let mut result = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("{{") {
        append(&mut result, &rest[..start])?;
        let Some(end) = rest[start + 2..].find("}}") else {
            append(&mut result, &rest[start..])?;
            return Ok(result);
        };
        let end = start + 2 + end;
        let key = &rest[start + 2..end];

        if let Some(value) = cache.get(key) {
            append(&mut result, value)?;
        } else if let Some(value) = defaults.get(key) {
            if stack.iter().any(|entry| entry == key) {
                return Err(format!(
                    "Postman collection variables contain a cycle involving {key:?}."
                ));
            }

            if stack.len() >= 64 {
                return Err("Postman collection variables are nested too deeply.".into());
            }

            stack.push(key.to_owned());
            let expanded = expand(value, defaults, cache, stack)?;
            append(&mut result, &expanded)?;
            cache.insert(key.to_owned(), expanded);
            stack.pop();
        } else {
            append(&mut result, &rest[start..end + 2])?;
        }

        rest = &rest[end + 2..];
    }

    append(&mut result, rest)?;
    Ok(result)
}

fn append(output: &mut String, text: &str) -> Result<(), String> {
    if output.len().saturating_add(text.len()) > 16 * 1024 * 1024 {
        return Err("An expanded Postman value exceeds the 16 MiB import limit.".into());
    }

    output.push_str(text);
    Ok(())
}
