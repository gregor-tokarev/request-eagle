use std::collections::{BTreeMap, HashMap};

use environment::VariableScopes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Variables {
    /// `pm.variables` overrides, which last for one execution.
    pub values: BTreeMap<String, String>,
    #[serde(flatten)]
    pub scopes: VariableScopes,
    pub generated: BTreeMap<String, String>,
}

impl Variables {
    /// The values `{{name}}` resolves to, with the overrides over the scopes.
    pub fn visible(&self) -> HashMap<String, String> {
        let mut values = self.scopes.values();
        values.extend(self.values.clone());

        values
    }
}

pub(super) use environment::generate_variable as dynamic_variable;
