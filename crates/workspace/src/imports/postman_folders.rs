use std::collections::{HashMap, HashSet};

use serde_json::Value;

pub(super) fn sibling_names(items: &[Value]) -> Vec<String> {
    // Reserve source folder names before assigning suffixes so a duplicate
    // cannot take the name of a later sibling folder.
    let mut reserved = items
        .iter()
        .filter(|item| item.get("item").is_some_and(Value::is_array))
        .map(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .unwrap_or("Imported request")
                .to_owned()
        })
        .collect::<HashSet<_>>();
    let mut next_suffixes = HashMap::new();

    items
        .iter()
        .map(|item| {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Imported request");

            if !item.get("item").is_some_and(Value::is_array) {
                return name.to_owned();
            }

            let next_suffix = next_suffixes.entry(name).or_insert(1);

            if *next_suffix == 1 {
                *next_suffix = 2;
                return name.to_owned();
            }

            loop {
                let candidate = format!("{name} ({next_suffix})");
                *next_suffix += 1;

                if reserved.insert(candidate.clone()) {
                    return candidate;
                }
            }
        })
        .collect()
}
