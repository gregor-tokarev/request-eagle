//! Applies a freshly serialized document onto the file as the user last saved
//! it, keeping comments, formatting and unknown fields.

use toml_edit::{Array, Item, TableLike, Value};

/// Merges `updates` into `target`, leaving unchanged values and their
/// decoration untouched.
pub(crate) fn merge_table(target: &mut dyn TableLike, updates: &dyn TableLike) {
    for (key, update) in updates.iter() {
        if let Some(current) = target.get_mut(key) {
            merge_item(current, update);
        } else {
            target.insert(key, update.clone());
        }
    }
}

fn merge_item(target: &mut Item, update: &Item) {
    if let (Some(target), Some(update)) = (target.as_table_like_mut(), update.as_table_like()) {
        merge_table(target, update);

        return;
    }

    match (target, update) {
        (Item::Value(target), Item::Value(update)) => merge_value(target, update),
        (target, update) => *target = update.clone(),
    }
}

fn merge_value(target: &mut Value, update: &Value) {
    match (target, update) {
        (Value::String(target), Value::String(update)) if target.value() == update.value() => {}
        (Value::Integer(target), Value::Integer(update)) if target.value() == update.value() => {}
        (Value::Float(target), Value::Float(update)) if target.value() == update.value() => {}
        (Value::Boolean(target), Value::Boolean(update)) if target.value() == update.value() => {}
        (Value::Datetime(target), Value::Datetime(update)) if target.value() == update.value() => {}
        (Value::Array(target), Value::Array(update)) => {
            if target
                .iter()
                .chain(update.iter())
                .all(|value| key_value(value).is_some())
            {
                merge_key_value_rows(target, update);

                return;
            }

            while target.len() > update.len() {
                target.remove(target.len() - 1);
            }

            for (index, value) in update.iter().enumerate() {
                if let Some(current) = target.get_mut(index) {
                    merge_value(current, value);
                } else {
                    target.push_formatted(value.clone());
                }
            }
        }
        (Value::InlineTable(target), Value::InlineTable(update)) => merge_table(target, update),
        (target, update) => {
            let decor = target.decor().clone();
            *target = update.clone();
            *target.decor_mut() = decor;
        }
    }
}

/// A header, parameter or metadata row's key and value. Rows are
/// `[key, value]`, or tables when they are switched off or described.
fn key_value(value: &Value) -> Option<(&str, &str)> {
    if let Some(row) = value.as_inline_table() {
        return Some((row.get("key")?.as_str()?, row.get("value")?.as_str()?));
    }

    let values = value.as_array()?;
    if values.len() != 2 {
        return None;
    }

    Some((values.get(0)?.as_str()?, values.get(1)?.as_str()?))
}

fn merge_key_value_rows(target: &mut Array, update: &Array) {
    let mut rows: Vec<_> = target
        .iter()
        .cloned()
        .map(|value| Some((value, String::new())))
        .collect();

    // TOML attaches comments after a comma to the next value or the array tail.
    // Associate same-line comments with their preceding row before moving rows.
    for index in 1..rows.len() {
        let (value, _) = rows[index].as_mut().unwrap();
        let prefix = value
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .unwrap_or_default()
            .to_owned();
        let (comment, prefix) = split_row_comment(&prefix);
        value.decor_mut().set_prefix(prefix);
        rows[index - 1].as_mut().unwrap().1 = comment.to_owned();
    }

    let trailing = target.trailing().as_str().unwrap_or_default().to_owned();
    let trailing = if let Some(Some((_, comment))) = rows.last_mut() {
        let (row_comment, trailing) = split_row_comment(&trailing);
        *comment = row_comment.to_owned();
        trailing
    } else {
        &trailing
    };

    // Reserve exact matches first so duplicate keys retain their own annotations.
    let mut matched: Vec<_> = update
        .iter()
        .map(|value| {
            let index = rows.iter().position(|row| {
                row.as_ref()
                    .is_some_and(|(current, _)| key_value(current) == key_value(value))
            })?;

            rows[index].take()
        })
        .collect();

    target.clear();
    let mut preceding_comment = String::new();

    for (value, matched) in update.iter().zip(&mut matched) {
        if matched.is_none() {
            let key = key_value(value).unwrap().0;
            if let Some(index) = rows.iter().position(|row| {
                row.as_ref()
                    .is_some_and(|(current, _)| key_value(current).unwrap().0 == key)
            }) {
                *matched = rows[index].take();
            }
        }

        let (mut current, following_comment) = matched
            .take()
            .unwrap_or_else(|| (value.clone(), String::new()));
        // Serialization omits a row's flag and description when they are
        // unset, so drop the ones the row had before.
        if let (Some(current), Some(value)) =
            (current.as_inline_table_mut(), value.as_inline_table())
        {
            for key in ["disabled", "description"] {
                if !value.contains_key(key) {
                    current.remove(key);
                }
            }
        }
        merge_value(&mut current, value);
        let prefix = current
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .unwrap_or_default();
        let prefix = join_row_comment(&preceding_comment, prefix);
        current.decor_mut().set_prefix(prefix);
        target.push_formatted(current);
        preceding_comment = following_comment;
    }

    target.set_trailing(join_row_comment(&preceding_comment, trailing));
}

fn split_row_comment(text: &str) -> (&str, &str) {
    let end = text.find('\n').unwrap_or(text.len());
    if text[..end].contains('#') {
        text.split_at(end)
    } else {
        ("", text)
    }
}

fn join_row_comment(comment: &str, following: &str) -> String {
    if !comment.is_empty() && !following.starts_with(['\r', '\n']) {
        format!("{comment}\n{following}")
    } else {
        format!("{comment}{following}")
    }
}
