use anyhow::{Context as _, Result, bail};
use collection::{CollectionRegistry, Entry, FileEntry, MovePlacement, Request};
use serde_json::{Value, json};
use std::{fs, path::Path};

use crate::commands::{Command, Placement, RequestInput};

pub fn load(root: &Path) -> Result<(CollectionRegistry, fs::File)> {
    // Serialize CLI edits before loading the registry, so parallel invocations
    // cannot save stale ordering or act on a moved/deleted snapshot.
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".cli.lock"))?;
    lock.try_lock()
        .context("Collections are being used by another CLI command; retry")?;
    let registry = CollectionRegistry::from_path(root)?;
    validate_paths(&registry)?;
    Ok((registry, lock))
}

pub fn dispatch(root: &Path, command: Command) -> Result<Value> {
    let (mut registry, _lock) = load(root)?;

    let path = match command {
        Command::CollectionsList {} => return Ok(json!(registry.collections().iter().map(|collection| {
            json!({"path": collection.path, "name": collection.path.file_name().unwrap_or_default().to_string_lossy()})
        }).collect::<Vec<_>>())),
        Command::CollectionsGet { path } => {
            let collection = registry.collections().iter().find(|entry| entry.path == path)
                .context("Unknown collection path")?;
            return Ok(json!({"path": path, "entries": entries(&collection.entries)}));
        }
        Command::RequestsList { collection, query } => {
            if let Some(path) = &collection
                && !registry.collections().iter().any(|entry| entry.path == *path) {
                bail!("Unknown collection path");
            }

            let mut output = Vec::new();
            for entry in registry.collections().iter().filter(|entry| collection.as_ref().is_none_or(|path| *path == entry.path)) {
                list_requests(&entry.entries, &entry.path, &query.to_lowercase(), &mut output);
            }
            return Ok(json!(output));
        }
        Command::RequestsGet { path } => return Ok(request_json(registry.file(&path).context("Unknown saved request path")?)),
        Command::CollectionsCreate {} => registry.create_collection()?,
        Command::FoldersCreate { parent } => registry.create_folder(&parent)?,
        Command::RequestsCreate { parent, name, request } => {
            let path = registry.create_request_with(&parent, &name, request::HttpRequest::from(request).into())?;
            return Ok(request_json(registry.file(&path).context("Created request was not found")?));
        }
        Command::RequestsUpdate { path, expected_id, request } => {
            registry.update_request(&path, &expected_id, request::HttpRequest::from(request).into())?;
            return Ok(request_json(registry.file(&path).context("Updated request was not found")?));
        }
        Command::EntriesRename { path, name } => registry.rename(&path, &name)?,
        Command::EntriesMove { path, target, placement } => registry.move_entry(&path, &target, match placement {
            Placement::Before => MovePlacement::Before,
            Placement::After => MovePlacement::After,
            Placement::Inside => MovePlacement::Inside,
        })?,
        Command::EntriesDelete { path, confirm } => {
            if !confirm { bail!("Set confirm=true to permanently delete this saved entry"); }
            registry.delete(&path)?;
            path
        }
        _ => bail!("Expected a collection operation"),
    };

    Ok(json!({"path": path}))
}

pub fn request_json(file: &FileEntry) -> Value {
    let Request::Http(request) = &file.request;
    json!({"path": file.path, "id": file.id, "name": file.name, "request": RequestInput::from(request)})
}

fn entries(items: &[Entry]) -> Vec<Value> {
    items.iter().map(|entry| match entry {
        Entry::File(file) => {
            let mut value = request_json(file);
            value["kind"] = json!("request");
            value
        }
        Entry::Directory(folder) => json!({"kind": "folder", "path": folder.path, "name": folder.name, "entries": entries(&folder.entries)}),
    }).collect()
}

fn list_requests(items: &[Entry], collection: &Path, query: &str, output: &mut Vec<Value>) {
    for entry in items {
        match entry {
            Entry::Directory(folder) => list_requests(&folder.entries, collection, query, output),
            Entry::File(file) => {
                let Request::Http(request) = &file.request;
                let value = json!({"path": file.path, "id": file.id, "name": file.name, "method": request.method, "url": request.path, "collection": collection});
                if query.is_empty() || value.to_string().to_lowercase().contains(query) {
                    output.push(value);
                }
            }
        }
    }
}

pub fn validate_paths(registry: &CollectionRegistry) -> Result<()> {
    fn visit(entries: &[Entry]) -> Result<()> {
        for entry in entries {
            entry
                .path()
                .to_str()
                .context("Collection paths must be valid UTF-8")?;
            if let Entry::Directory(folder) = entry {
                visit(&folder.entries)?;
            }
        }
        Ok(())
    }

    for collection in registry.collections() {
        collection
            .path
            .to_str()
            .context("Collection paths must be valid UTF-8")?;
        visit(&collection.entries)?;
    }
    Ok(())
}
