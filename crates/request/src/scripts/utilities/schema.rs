use std::collections::HashSet;

use jsonschema::PatternOptions;
use serde_json::{Value, json};

const DATA_LIMIT: usize = 1024 * 1024;
const SCHEMA_LIMIT: usize = 64 * 1024;
const ERROR_LIMIT: usize = 20;
const EXPANDED_SCHEMA_LIMIT: usize = 10_000;
const SCHEMA_DEPTH_LIMIT: usize = 32;
const EVALUATION_NODE_LIMIT: usize = 100_000;
const EVALUATION_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const REGEX_INPUT_LIMIT: usize = 4096;
const REGEX_WORK_LIMIT: usize = 4096;
const WORK_LIMIT_ERROR: &str = "Schema validation exceeds the combined data/schema work limit";

#[derive(Default)]
struct TreeCost {
    nodes: usize,
    path_bytes: usize,
    longest_string: usize,
    string_bytes: usize,
}

fn check_tree(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    path_bytes: usize,
) -> Result<TreeCost, String> {
    if depth == 0 || *nodes == 0 {
        return Err("JSON exceeds the validation depth or node limit".into());
    }

    *nodes -= 1;
    let mut cost = TreeCost {
        nodes: 1,
        path_bytes,
        longest_string: value.as_str().map_or(0, str::len),
        string_bytes: value.as_str().map_or(0, str::len),
    };
    let mut add = |child: TreeCost| -> Result<(), String> {
        cost.nodes += child.nodes;
        cost.path_bytes += child.path_bytes;
        cost.longest_string = cost.longest_string.max(child.longest_string);
        cost.string_bytes += child.string_bytes;

        if cost.path_bytes > EVALUATION_BYTE_LIMIT {
            return Err(WORK_LIMIT_ERROR.into());
        }

        Ok(())
    };

    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                add(check_tree(
                    value,
                    depth - 1,
                    nodes,
                    path_bytes + 1 + index.to_string().len(),
                )?)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let escaped_length = key.len()
                    + key
                        .bytes()
                        .filter(|byte| matches!(byte, b'~' | b'/'))
                        .count();
                let mut child =
                    check_tree(value, depth - 1, nodes, path_bytes + 1 + escaped_length)?;
                child.longest_string = child.longest_string.max(key.len());
                child.string_bytes += key.len();

                add(child)?;
            }
        }
        _ => {}
    }

    Ok(cost)
}

struct EvaluationBudget {
    data: TreeCost,
    data_bytes: usize,
    nodes_left: usize,
    bytes_left: usize,
    regex_left: usize,
}

impl EvaluationBudget {
    fn charge(&mut self, schema: &Value, ancestors: usize) -> Result<(), String> {
        // jsonschema eagerly gathers errors, including every failed anyOf
        // branch, before returning its iterator. An output error cap alone
        // therefore does not bound native allocations. Charge the entire JSON
        // subtree at every schema ancestor, and again for every reference use.
        // This counts literal enum/const payloads and repeated validity checks
        // in nested compositions as well as the eventual individual errors.
        let schema_cost = check_tree(schema, SCHEMA_DEPTH_LIMIT, &mut 1_000, 0)?;
        let schema_bytes = schema.to_string().len() + schema_cost.path_bytes;
        let nodes = self.data.nodes.saturating_mul(schema_cost.nodes);
        let bytes = self
            .data_bytes
            .saturating_mul(schema_cost.nodes)
            .saturating_add(self.data.nodes.saturating_mul(schema_bytes));

        self.nodes_left = self.nodes_left.checked_sub(nodes).ok_or(WORK_LIMIT_ERROR)?;
        self.bytes_left = self.bytes_left.checked_sub(bytes).ok_or(WORK_LIMIT_ERROR)?;

        // Regex-format parsing can be quadratic, and the linear pattern engine
        // can still cost pattern-state-count × input-length under DFA churn.
        // Include object keys for patternProperties and propertyNames.
        let regex_count =
            usize::from(schema.get("format").and_then(Value::as_str) == Some("regex"))
                + usize::from(schema.get("pattern").is_some())
                + schema
                    .get("patternProperties")
                    .and_then(Value::as_object)
                    .map_or(0, serde_json::Map::len);

        if regex_count > 0 && self.data.longest_string > REGEX_INPUT_LIMIT {
            return Err("Schemas using regular expressions require data strings and property names of at most 4096 bytes".into());
        }

        // A per-string cap alone still permits expensive matching of that
        // string against many patterns. Charge every string/key for each regex
        // occurrence and ancestor, accounting for nested composition rechecks.
        // The fixed allowance bounds compilation count when strings are empty.
        // Repeated references spend this budget again.
        let regex_work = self
            .data
            .string_bytes
            .saturating_add(64)
            .saturating_mul(regex_count)
            .saturating_mul(ancestors);
        self.regex_left = self
            .regex_left
            .checked_sub(regex_work)
            .ok_or("Schema validation exceeds the regular-expression work limit")?;

        Ok(())
    }
}

/// Validate reference expansion before entering the native validator. A small
/// schema can otherwise expand into an arbitrarily large reference graph.
fn check_schema(
    value: &Value,
    root: &Value,
    active: &mut HashSet<*const Value>,
    remaining: &mut usize,
    depth: usize,
    budget: &mut EvaluationBudget,
) -> Result<(), String> {
    if depth == 0 || *remaining == 0 {
        return Err("Schema exceeds the reference expansion limit".into());
    }

    *remaining -= 1;
    budget.charge(value, SCHEMA_DEPTH_LIMIT + 1 - depth)?;

    if !active.insert(value as *const Value) {
        return Err("Cyclic schema references are not supported".into());
    }

    if let Some(fields) = value.as_object() {
        if fields.contains_key("unevaluatedProperties") || fields.contains_key("unevaluatedItems") {
            return Err(
                "unevaluatedProperties and unevaluatedItems are not supported in script schemas"
                    .into(),
            );
        }

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

            check_schema(target, root, active, remaining, depth - 1, budget)?;
        }

        for (keyword, child) in fields {
            match keyword.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions"
                | "dependentSchemas" | "dependencies" => {
                    if let Some(children) = child.as_object() {
                        for child in children.values().filter(|child| !child.is_array()) {
                            check_schema(child, root, active, remaining, depth - 1, budget)?;
                        }
                    }
                }
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    if let Some(children) = child.as_array() {
                        for child in children {
                            check_schema(child, root, active, remaining, depth - 1, budget)?;
                        }
                    }
                }
                "items" if child.is_array() => {
                    for child in child.as_array().unwrap() {
                        check_schema(child, root, active, remaining, depth - 1, budget)?;
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
                | "contentSchema" => {
                    check_schema(child, root, active, remaining, depth - 1, budget)?;
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

    let data_bytes = data.len();
    let data: Value =
        serde_json::from_str(data).map_err(|error| format!("Invalid JSON data: {error}"))?;
    let schema: Value =
        serde_json::from_str(schema).map_err(|error| format!("Invalid JSON Schema: {error}"))?;

    let data_cost = check_tree(&data, 64, &mut 20_000, 0)?;
    check_tree(&schema, SCHEMA_DEPTH_LIMIT, &mut 1_000, 0)?;
    let mut remaining = EXPANDED_SCHEMA_LIMIT;
    let mut budget = EvaluationBudget {
        data_bytes: data_bytes + data_cost.path_bytes,
        data: data_cost,
        nodes_left: EVALUATION_NODE_LIMIT,
        bytes_left: EVALUATION_BYTE_LIMIT,
        regex_left: REGEX_WORK_LIMIT,
    };

    check_schema(
        &schema,
        &schema,
        &mut HashSet::new(),
        &mut remaining,
        SCHEMA_DEPTH_LIMIT,
        &mut budget,
    )?;

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
