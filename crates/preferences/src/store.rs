use crate::{AppearancePreferences, RequestPreferences};

use anyhow::{Context as _, Result, bail};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub appearance: AppearancePreferences,
    pub request: RequestPreferences,
}

impl Global for Preferences {}

#[derive(Default)]
struct Storage {
    path: Option<PathBuf>,
    load_error: Option<String>,
}

impl Global for Storage {}

pub fn init(cx: &mut App) {
    if !cx.has_global::<Preferences>() {
        cx.set_global(Preferences::default());
    }

    if !cx.has_global::<Storage>() {
        cx.set_global(Storage::default());
    }
}

pub fn load(directory: impl AsRef<Path>, cx: &mut App) -> Result<()> {
    init(cx);

    let path = directory.as_ref().join("preferences.json");
    cx.set_global(Storage {
        path: Some(path.clone()),
        load_error: None,
    });

    let result: Result<Preferences> = read(&path).and_then(|bytes| match bytes {
        Some(bytes) => serde_json::from_slice(&bytes).context("Invalid preferences.json"),
        None => Ok(Preferences::default()),
    });

    match result {
        Ok(preferences) => {
            cx.set_global(preferences);

            Ok(())
        }
        Err(error) => {
            cx.global_mut::<Storage>().load_error = Some(format!("{error:#}"));

            Err(error)
        }
    }
}

/// All preference changes use this path. Persist before publishing the new
/// global value, so observers only apply changes that were saved successfully.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Preferences)) -> Result<()> {
    init(cx);

    if let Some(error) = &cx.global::<Storage>().load_error {
        bail!("{error}. Fix the preferences file and reload before saving changes.");
    }

    let mut preferences = cx.try_global::<Preferences>().cloned().unwrap_or_default();
    change(&mut preferences);

    if let Some(path) = &cx.global::<Storage>().path {
        persist(path, &preferences)?;
    }

    cx.set_global(preferences);

    Ok(())
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Could not read {}", path.display())),
    }
}

fn persist(path: &Path, preferences: &Preferences) -> Result<()> {
    let parent = path.parent().context("Preferences path has no parent")?;
    fs::create_dir_all(parent)?;

    // A unique file in the same directory gives us an atomic replacement and
    // automatic cleanup on failure, without a shared .tmp filename.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(preferences)?)?;
    temporary.as_file().sync_all()?;

    temporary
        .persist(path)
        .with_context(|| format!("Could not save {}", path.display()))?;

    Ok(())
}
