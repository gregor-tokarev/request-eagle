use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Variables {
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    pub generated: BTreeMap<String, String>,
}

pub(super) use environment::generate_variable as dynamic_variable;
