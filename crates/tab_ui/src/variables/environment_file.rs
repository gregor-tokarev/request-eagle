use std::{collections::HashMap, fs, io::Write as _, path::Path};

use anyhow::Context as _;
use toml_edit::{DocumentMut, Item, Key, Value};

pub(super) fn save_entry(
    path: &Path,
    previous_name: Option<&str>,
    name: &str,
    value: Option<&str>,
) -> anyhow::Result<HashMap<String, String>> {
    let target = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Some(fs::canonicalize(path)?),
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let path = target.as_deref().unwrap_or(path);
    let parent = path
        .parent()
        .context("The environment file needs a parent directory.")?;
    fs::create_dir_all(parent)?;
    let mut lock_name = std::ffi::OsString::from(".");
    lock_name.push(
        path.file_name()
            .context("The environment file needs a name.")?,
    );
    lock_name.push(".lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join(lock_name))?;
    lock.lock()?;

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document = source.parse::<DocumentMut>()?;
    let mut entries = document
        .iter()
        .map(|(name, item)| {
            let value = item
                .as_str()
                .with_context(|| format!("Environment variable {name} must be a string."))?;
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;

    let mut value = value.map(str::to_owned);
    if let Some(previous) = previous_name.filter(|previous| *previous != name) {
        anyhow::ensure!(
            !entries.contains_key(name),
            "A variable with that name already exists."
        );
        let current = entries
            .remove(previous)
            .context("The selected variable was removed. Reload its source and retry.")?;
        let order: HashMap<_, _> = document
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (if key == previous { name } else { key }.to_owned(), index))
            .collect();
        if let Some((old_key, item)) = document.remove_entry(previous) {
            let key = Key::new(name)
                .with_leaf_decor(old_key.leaf_decor().clone())
                .with_dotted_decor(old_key.dotted_decor().clone());
            document.insert_formatted(&key, item);
            document.sort_values_by(|left, _, right, _| order[left.get()].cmp(&order[right.get()]));
        }
        entries.insert(name.to_owned(), current.clone());
        // A rename without an edited value moves the latest stored value.
        value = Some(value.unwrap_or(current));
    }
    if let Some(value) = value {
        if entries.get(name) != Some(&value) {
            let mut replacement = Value::from(value.clone());
            if let Some(previous) = document.get(name).and_then(Item::as_value) {
                *replacement.decor_mut() = previous.decor().clone();
            }
            document[name] = Item::Value(replacement);
        }
        entries.insert(name.to_owned(), value);
    } else {
        document.remove(name);
        entries.remove(name);
    }

    let permissions = match fs::metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                !metadata.permissions().readonly(),
                "The environment file is read-only."
            );
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(document.to_string().as_bytes())?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    let current = match fs::read_to_string(path) {
        Ok(current) => current,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        current == source,
        "The environment file changed while saving. Reload it and retry."
    );
    temporary.persist(path)?;
    Ok(entries)
}
