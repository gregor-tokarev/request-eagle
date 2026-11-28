use std::{collections::BTreeMap, io::Write, path::Path};

use gpui_kit::OperatingSystem;

use crate::Binding;

pub(super) fn load(
    path: &Path,
    os: OperatingSystem,
) -> anyhow::Result<BTreeMap<String, Option<String>>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(error) => return Err(error.into()),
    };

    let mut overrides: BTreeMap<String, Option<String>> = serde_json::from_str(&contents)?;
    for keystrokes in overrides.values_mut().flatten() {
        *keystrokes = Binding::new(keystrokes, None, os)?.keystrokes;
    }

    Ok(overrides)
}

pub(super) fn save(
    path: &Path,
    overrides: &BTreeMap<String, Option<String>>,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid key bindings path"))?;
    std::fs::create_dir_all(parent)?;

    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut file, overrides)?;
    file.write_all(b"\n")?;

    file.as_file().sync_all()?;
    file.persist(path)?;

    Ok(())
}
