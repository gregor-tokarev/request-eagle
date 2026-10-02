use anyhow::{Context as _, Result, bail};
use collection::{CollectionRegistry, Entry, FileEntry, MovePlacement};
use request::Request;
use serde_json::{Value, json};
use std::{fs, path::Path};

use crate::commands::{Command, GrpcRequestInput, Placement, RequestInput, SavedRequest};

pub fn load(root: &Path) -> Result<CollectionRegistry> {
    let registry = CollectionRegistry::from_path(root);
    // The app leaves unreadable files out and only counts them, so the CLI
    // is where agents learn which files to fix and why. Commands must not act
    // on an incomplete view of the collections.
    if !registry.skipped().is_empty() {
        let files = registry
            .skipped()
            .iter()
            .map(|skipped| format!("{}\n{}", skipped.path.display(), skipped.error))
            .collect::<Vec<_>>()
            .join("\n\n");
        bail!("Could not load these files. Fix or remove them first.\n\n{files}");
    }
    validate_paths(&registry)?;
    Ok(registry)
}

pub(crate) fn lock_for_edit(root: &Path) -> Result<fs::File> {
    // Serialize CLI edits before loading the registry, so parallel invocations
    // cannot save stale ordering or act on a moved/deleted snapshot.
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".cli.lock"))?;
    lock.try_lock()
        .context("Collections are being used by another CLI command; retry")?;
    Ok(lock)
}

pub fn dispatch(root: &Path, command: Command) -> Result<Value> {
    let _lock = match &command {
        Command::CollectionsList {}
        | Command::CollectionsGet { .. }
        | Command::RequestsList { .. }
        | Command::RequestsGet { .. } => None,
        _ => Some(lock_for_edit(root)?),
    };
    let mut registry = load(root)?;

    let path = match command {
        Command::CollectionsList {} => return Ok(json!(registry.collections().iter().map(|collection| {
            json!({"path": collection.path, "name": collection.path.file_name().unwrap_or_default().to_string_lossy()})
        }).collect::<Vec<_>>())),
        Command::CollectionsGet { path } => {
            let collection = registry.collections().iter().find(|entry| entry.path == path)
                .context("Unknown collection path")?;
            let scripts = collection.scripts();
            return Ok(json!({
                "path": path,
                "variables": collection.local_env().entries,
                "scripts": {"pre_request": scripts.pre_request, "post_response": scripts.post_response},
                "auth": collection.auth(),
                "entries": entries(&collection.entries),
            }));
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
            let path = registry.create_request_with(&parent, &name, request.into())?;
            return Ok(request_json(registry.file(&path).context("Created request was not found")?));
        }
        Command::RequestsUpdate { path, expected_id, request } => {
            if let Some(Request::WebSocket(_)) = registry.file(&path).map(|file| &file.request) {
                bail!("requests.update edits HTTP and gRPC requests, and this is a WebSocket request");
            }
            registry.update_request(&path, &expected_id, request.into())?;
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

fn request_json(file: &FileEntry) -> Value {
    match &file.request {
        Request::Http(request) => {
            json!({"path": file.path, "id": file.id, "name": file.name, "request": RequestInput::from(request)})
        }
        Request::Grpc(request) => {
            json!({"path": file.path, "id": file.id, "name": file.name, "request": GrpcRequestInput::from(request)})
        }
        Request::WebSocket(request) => {
            json!({"path": file.path, "id": file.id, "name": file.name, "websocket": request})
        }
    }
}

impl From<SavedRequest> for Request {
    fn from(request: SavedRequest) -> Self {
        match request {
            SavedRequest::Http(request) => request::HttpRequest::from(request).into(),
            SavedRequest::Grpc(request) => request::GrpcRequest::from(request).into(),
        }
    }
}

fn entries(items: &[Entry]) -> Vec<Value> {
    items.iter().map(|entry| match entry {
        Entry::File(file) => {
            let mut value = request_json(file);
            value["kind"] = json!("request");
            value
        }
        Entry::Flow(flow) => json!({"kind": "flow", "path": flow.path, "id": flow.id, "name": flow.name}),
        Entry::Directory(folder) => json!({"kind": "folder", "path": folder.path, "name": folder.name, "entries": entries(&folder.entries)}),
    }).collect()
}

fn list_requests(items: &[Entry], collection: &Path, query: &str, output: &mut Vec<Value>) {
    for entry in items {
        match entry {
            Entry::Directory(folder) => list_requests(&folder.entries, collection, query, output),
            Entry::Flow(_) => {}
            Entry::File(file) => {
                let value = match &file.request {
                    Request::Http(request) => {
                        json!({"path": file.path, "id": file.id, "name": file.name, "method": request.method, "url": request.path, "collection": collection})
                    }
                    Request::Grpc(request) => {
                        json!({"path": file.path, "id": file.id, "name": file.name, "protocol": "grpc", "method": request.method, "url": request.url, "collection": collection})
                    }
                    Request::WebSocket(request) => {
                        json!({"path": file.path, "id": file.id, "name": file.name, "protocol": "websocket", "url": request.url, "collection": collection})
                    }
                };
                if query.is_empty() || value.to_string().to_lowercase().contains(query) {
                    output.push(value);
                }
            }
        }
    }
}

fn validate_paths(registry: &CollectionRegistry) -> Result<()> {
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
