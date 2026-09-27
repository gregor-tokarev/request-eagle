use std::collections::HashSet;

use jsonschema::PatternOptions;
use serde_json::{Value, json};

const DATA_LIMIT: usize = 1024 * 1024;
const SCHEMA_LIMIT: usize = 64 * 1024;
const ERROR_LIMIT: usize = 20;
const EXPANDED_SCHEMA_LIMIT: usize = 10_000;

fn check_tree(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    if depth == 0 || *nodes == 0 {
        return Err("JSON exceeds the validation depth or node limit".into());
    }

    *nodes -= 1;

    match value {
        Value::Array(values) => {
            for value in values {
                check_tree(value, depth - 1, nodes)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                check_tree(value, depth - 1, nodes)?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Validate reference expansion before entering the native validator. A small
/// schema can otherwise expand into an arbitrarily large reference graph.
fn check_schema(
    value: &Value,
    root: &Value,
    active: &mut HashSet<*const Value>,
    remaining: &mut usize,
    depth: usize,
) -> Result<(), String> {
    if depth == 0 || *remaining == 0 {
        return Err("Schema exceeds the reference expansion limit".into());
    }

    *remaining -= 1;

    if !active.insert(value as *const Value) {
        return Err("Cyclic schema references are not supported".into());
    }

    if let Some(fields) = value.as_object() {
        if fields.contains_key("$dynamicRef") || fields.contains_key("$recursiveRef") {
            return Err("Dynamic and recursive schema references are not supported".into());
        }

        if !std::ptr::eq(value, root) && (fields.contains_key("$id") || fields.contains_key("id")) {
            return Err("Schema identifiers are only supported at the root".into());
        }

        if let Some(reference) = fields.get("$ref").and_then(Value::as_str) {
            let pointer = reference.strip_prefix('#').ok_or_else(|| {
                "External schema references are not supported; use local #/$defs/... references"
                    .to_owned()
            })?;

            let pointer = percent_encoding::percent_decode_str(pointer)
                .decode_utf8()
                .map_err(|_| "Invalid UTF-8 in schema reference".to_owned())?;

            if !pointer.is_empty() && !pointer.starts_with('/') {
                return Err("Schema references must use local JSON pointers".into());
            }

            let target = root
                .pointer(&pointer)
                .ok_or_else(|| "Schema reference does not exist".to_owned())?;

            check_schema(target, root, active, remaining, depth - 1)?;
        }

        for (keyword, child) in fields {
            match keyword.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions"
                | "dependentSchemas" | "dependencies" => {
                    if let Some(children) = child.as_object() {
                        for child in children.values().filter(|child| !child.is_array()) {
                            check_schema(child, root, active, remaining, depth - 1)?;
                        }
                    }
                }
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    if let Some(children) = child.as_array() {
                        for child in children {
                            check_schema(child, root, active, remaining, depth - 1)?;
                        }
                    }
                }
                "items" if child.is_array() => {
                    for child in child.as_array().unwrap() {
                        check_schema(child, root, active, remaining, depth - 1)?;
                    }
                }
                "items"
                | "additionalItems"
                | "additionalProperties"
                | "contains"
                | "propertyNames"
                | "not"
                | "if"
                | "then"
                | "else"
                | "unevaluatedItems"
                | "unevaluatedProperties"
                | "contentSchema" => {
                    check_schema(child, root, active, remaining, depth - 1)?;
                }
                _ => {}
            }
        }
    }

    active.remove(&(value as *const Value));

    Ok(())
}

pub(super) fn validate(data: &str, schema: &str) -> Result<String, String> {
    if data.len() > DATA_LIMIT {
        return Err("Schema validation data cannot exceed 1 MiB".into());
    }

    if schema.len() > SCHEMA_LIMIT {
        return Err("JSON Schema cannot exceed 64 KiB".into());
    }

    let data: Value =
        serde_json::from_str(data).map_err(|error| format!("Invalid JSON data: {error}"))?;
    let schema: Value =
        serde_json::from_str(schema).map_err(|error| format!("Invalid JSON Schema: {error}"))?;

    check_tree(&data, 64, &mut 20_000)?;
    check_tree(&schema, 32, &mut 1_000)?;
    let mut remaining = EXPANDED_SCHEMA_LIMIT;

    check_schema(&schema, &schema, &mut HashSet::new(), &mut remaining, 32)?;

    let validator = jsonschema::options()
        .offline()
        .should_validate_formats(true)
        .with_pattern_options(
            PatternOptions::regex()
                .size_limit(256 * 1024)
                .dfa_size_limit(256 * 1024),
        )
        .build(&schema)
        .map_err(|error| {
            format!(
                "Invalid JSON Schema: {}",
                error.to_string().chars().take(2048).collect::<String>()
            )
        })?;

    let mut failures = validator.iter_errors(&data);
    let errors = failures
        .by_ref()
        .take(ERROR_LIMIT)
        .map(|error| {
            json!({
                "instancePath": error.instance_path().as_str(),
                "schemaPath": error.schema_path().as_str(),
                "message": error.to_string().chars().take(1024).collect::<String>(),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "truncated": failures.next().is_some(),
    })
    .to_string())
}
