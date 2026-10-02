use std::collections::{BTreeMap, HashMap};

use environment::VariableScopes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Variables {
    /// `pm.variables` overrides, which last for one execution.
    pub values: BTreeMap<String, String>,
    /// The Collection Runner's data file row, `pm.iterationData`, with the
    /// values a JSON file gives them.
    #[serde(default)]
    pub data: BTreeMap<String, serde_json::Value>,
    #[serde(flatten)]
    pub scopes: VariableScopes,
    pub generated: BTreeMap<String, String>,
}

impl Variables {
    /// Take the variables a script left. The data row keeps its own values:
    /// a script can unset them, but its changes to them, or JavaScript's
    /// rounding of large numbers, do not reach the request.
    pub fn update(&mut self, mut output: Variables) {
        let data = std::mem::take(&mut self.data);
        output.data = data
            .into_iter()
            .filter(|(name, _)| output.data.contains_key(name))
            .collect();
        *self = output;
    }

    /// How `{{name}}` writes each data file value, which scripts substitute
    /// too, so both write the same text.
    pub fn data_texts(&self) -> BTreeMap<String, String> {
        self.data
            .iter()
            .map(|(name, value)| (name.clone(), data_text(value)))
            .collect()
    }

    /// The values `{{name}}` resolves to: the overrides over the data row,
    /// over the scopes.
    pub fn visible(&self) -> HashMap<String, String> {
        let mut values = self.scopes.values();
        values.extend(
            self.data
                .iter()
                .map(|(name, value)| (name.clone(), data_text(value))),
        );
        values.extend(self.values.clone());

        values
    }
}

pub(super) use environment::generate_variable as dynamic_variable;

/// A data file value as `{{name}}` writes it: text as it is, other values as
/// JSON.
fn data_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        value => value.to_string(),
    }
}
