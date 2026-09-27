use crate::{CollectionPanel, CollectionPanelEvent};
use collection::{Entry, MovePlacement};
use gpui_kit::{Context, Window};
use request_eagle_automation::{Command, Placement};
use serde_json::{Value, json};
use std::path::Path;

impl CollectionPanel {
    pub fn registry(&self) -> &collection::CollectionRegistry {
        &self.collections
    }

    pub fn automation_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(Value, Vec<CollectionPanelEvent>), String> {
        let result = self.apply_automation(command, window, cx);
        result.map_err(|error| error.to_string())
    }

    fn apply_automation(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(Value, Vec<CollectionPanelEvent>), collection::CollectionEditError> {
        let mut renamed = None;
        let path = match command {
            Command::CollectionsList { query } => {
                return self.list_entries(&query).map(|value| (value, Vec::new()));
            }
            Command::RequestsGet { path } => {
                let file = self
                    .collections
                    .file(&path)
                    .ok_or(collection::CollectionEditError::NotFound)?;
                let collection::Request::Http(request) = &file.request;
                return Ok((
                    json!({"path": path, "id": file.id, "name": file.name, "request": request_eagle_automation::RequestInput::from(request)}),
                    Vec::new(),
                ));
            }
            Command::CollectionsCreate {} => {
                if let Some(directory) = self.collections.directory() {
                    require_utf8_path(directory)?;
                }
                self.collections.create_collection()?
            }
            Command::FoldersCreate { parent } => self.collections.create_folder(&parent)?,
            Command::RequestsCreate {
                parent,
                name,
                request,
            } => {
                let request: collection::HttpRequest = request.into();
                self.save_new_request(&parent, &name, request.into(), window, cx)?
                    .path
            }
            Command::EntriesRename { path, name } => {
                let destination = self.collections.rename(&path, &name)?;
                renamed = Some((path, destination.clone()));
                destination
            }
            Command::EntriesMove {
                path,
                target,
                placement,
            } => {
                let placement = match placement {
                    Placement::Before => MovePlacement::Before,
                    Placement::After => MovePlacement::After,
                    Placement::Inside => MovePlacement::Inside,
                };
                let destination = self.collections.move_entry(&path, &target, placement)?;
                renamed = Some((path, destination.clone()));
                destination
            }
            Command::EntriesDelete {
                path,
                confirm: true,
            } => {
                self.collections.delete(&path)?;
                path
            }
            _ => return Err(collection::CollectionEditError::NotFound),
        };

        self.error = None;
        self.rename = None;
        self.pending_delete = None;
        let relocations = self.rebuild_tree_with_relocations(
            Some(&path),
            renamed
                .as_ref()
                .map(|(old, new)| (old.as_path(), new.as_path())),
            cx,
        );
        Ok((json!({"path": path}), relocations))
    }

    fn list_entries(&self, query: &str) -> Result<Value, collection::CollectionEditError> {
        fn visit(
            entries: &[Entry],
            collection: &Path,
            query: &str,
            output: &mut Vec<Value>,
        ) -> Result<(), collection::CollectionEditError> {
            for entry in entries {
                require_utf8_path(entry.path())?;

                let value = match entry {
                    Entry::Directory(folder) => {
                        visit(&folder.entries, collection, query, output)?;
                        json!({"kind": "folder", "path": folder.path, "name": folder.name, "collection": collection})
                    }
                    Entry::File(file) => {
                        let collection::Request::Http(request) = &file.request;
                        json!({"kind": "request", "path": file.path, "id": file.id, "name": file.name, "collection": collection, "method": request.method, "url": request.path})
                    }
                };
                if query.is_empty() || value.to_string().to_lowercase().contains(query) {
                    output.push(value);
                }
            }

            Ok(())
        }

        let query = query.to_lowercase();
        let mut entries = Vec::new();
        for collection in self.collections.collections() {
            require_utf8_path(&collection.path)?;

            let name = collection
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if query.is_empty() || name.to_lowercase().contains(&query) {
                entries.push(json!({"kind": "collection", "path": collection.path, "name": name}));
            }
            visit(&collection.entries, &collection.path, &query, &mut entries)?;
        }
        Ok(json!(entries))
    }
}

fn require_utf8_path(path: &Path) -> Result<(), collection::CollectionEditError> {
    if path.to_str().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Collection paths must be valid UTF-8 for CLI commands",
        )
        .into());
    }

    Ok(())
}
