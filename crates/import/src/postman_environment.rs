//! Postman environments: the JSON files Postman exports, and the YAML files it
//! writes in `postman/environments` for a workspace connected to Git.

use std::collections::HashMap;

use serde_json::Value;

use crate::document::{clean_name, text};

/// An environment's name and the values of its enabled variables.
pub struct ImportedEnvironment {
    pub name: String,
    pub variables: HashMap<String, String>,
}

/// Whether the document is an environment. Exported globals have the same
/// shape, and import as an environment too.
pub(crate) fn is_environment(document: &Value) -> bool {
    document["name"].is_string() && document["values"].is_array()
}

pub(crate) fn convert(document: &Value) -> ImportedEnvironment {
    let variables = document["values"]
        .as_array()
        .into_iter()
        .flatten()
        // Postman turns a variable off with either flag.
        .filter(|variable| {
            variable["enabled"].as_bool() != Some(false)
                && variable["disabled"].as_bool() != Some(true)
        })
        .filter_map(|variable| {
            let key = variable["key"].as_str().filter(|key| !key.is_empty())?;
            Some((key.to_owned(), text(variable.get("value"))))
        })
        .collect();

    ImportedEnvironment {
        name: clean_name(document["name"].as_str(), "Postman Environment"),
        variables,
    }
}
