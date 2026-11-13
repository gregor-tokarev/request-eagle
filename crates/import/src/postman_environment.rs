//! Postman environments: the JSON files Postman exports, and the YAML files it
//! writes in `postman/environments` for a workspace connected to Git.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::document::{clean_name, text};

/// An environment's name and the values of its enabled variables.
pub struct ImportedEnvironment {
    pub name: String,
    pub variables: HashMap<String, String>,
    /// Variables that Postman marks as secret.
    pub secrets: HashSet<String>,
}

/// Whether the document is an environment. Exported globals have the same
/// shape, and import as an environment too.
pub(crate) fn is_environment(document: &Value) -> bool {
    document["name"].is_string() && document["values"].is_array()
}

pub(crate) fn convert(document: &Value) -> ImportedEnvironment {
    let mut variables = HashMap::new();
    let mut secrets = HashSet::new();

    let enabled = document["values"]
        .as_array()
        .into_iter()
        .flatten()
        // Postman turns a variable off with either flag.
        .filter(|variable| {
            variable["enabled"].as_bool() != Some(false)
                && variable["disabled"].as_bool() != Some(true)
        });

    for variable in enabled {
        let Some(key) = variable["key"].as_str().filter(|key| !key.is_empty()) else {
            continue;
        };

        variables.insert(key.to_owned(), text(variable.get("value")));

        if variable["type"].as_str() == Some("secret") {
            secrets.insert(key.to_owned());
        } else {
            // A later variable with the same key replaces an earlier one.
            secrets.remove(key);
        }
    }

    ImportedEnvironment {
        name: clean_name(document["name"].as_str(), "Postman Environment"),
        variables,
        secrets,
    }
}
