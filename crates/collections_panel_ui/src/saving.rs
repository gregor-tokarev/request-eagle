use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::{CollectionEditError, FileEntry};

use gpui_kit::{Context, SharedString, Window};
use request::{Request, RequestScripts};

use super::{CollectionPanel, tree::ItemKind};

/// A collection or folder that can hold a newly saved request.
#[derive(Clone)]
pub struct SaveDestination {
    pub path: PathBuf,
    /// The collection's or folder's own name.
    pub name: SharedString,
    pub collection: SharedString,
    pub folders: Vec<SharedString>,
}

impl CollectionPanel {
    /// Save the editor's request and refresh the snapshot used when reopening it.
    pub fn save_request(
        &mut self,
        path: &Path,
        expected_id: &str,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Result<(), CollectionEditError> {
        self.collections
            .update_request(path, expected_id, request)?;
        let file = self.collections.file(path).expect("saved request exists");

        if self.tree.request_changed(file) {
            // Cancel any result computed from the previous search documents.
            self.rows_task = None;
            Arc::make_mut(&mut self.tree).update_request(file);
            self.refresh_rows(false, cx);
        }

        Ok(())
    }

    /// Save a flow tab's blocks and connections.
    pub fn save_flow(
        &mut self,
        path: &Path,
        expected_id: &str,
        flow: flow::Flow,
    ) -> Result<(), CollectionEditError> {
        self.collections.update_flow(path, expected_id, flow)
    }

    /// Save a collection tab's edits, renaming its directory when the name
    /// changed. Returns the collection's path after the save.
    pub fn save_collection(
        &mut self,
        path: &Path,
        name: &str,
        variables: HashMap<String, String>,
        scripts: RequestScripts,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        // Rename first: an invalid or taken name then fails before any file
        // changes. The rename event keeps the tab in step if a later write fails.
        let destination = if path.file_name().is_some_and(|current| current == name) {
            path.to_path_buf()
        } else {
            let destination = self.collections.rename(path, name)?;
            let selected = self.selected.map(|index| {
                let selected = &self.tree.items[index].path;
                match selected.strip_prefix(path) {
                    Ok(relative) => destination.join(relative),
                    Err(_) => selected.clone(),
                }
            });
            self.rebuild_tree(selected.as_deref(), Some((path, &destination)), cx);

            destination
        };

        self.collections
            .update_collection(&destination, variables, scripts)?;

        Ok(destination)
    }

    pub fn save_destinations(&self) -> Vec<SaveDestination> {
        self.tree
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.is_branch())
            .map(|(index, item)| {
                let (collection, mut folders) = self.tree.location(index);
                if item.kind == ItemKind::Folder {
                    folders.push(item.label.clone());
                }

                SaveDestination {
                    path: item.path.clone(),
                    name: item.label.clone(),
                    collection,
                    folders,
                }
            })
            .collect()
    }

    pub fn create_save_collection(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.collections.create_collection()?;
        self.rebuild_tree(Some(&path), None, cx);

        Ok(path)
    }

    pub fn save_new_request(
        &mut self,
        parent: &Path,
        name: &str,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<FileEntry, CollectionEditError> {
        let path = self
            .collections
            .create_request_with(parent, name, request)?;
        let file = self
            .collections
            .file(&path)
            .expect("created request exists")
            .clone();
        self.reveal(&path, None, window, cx);

        Ok(file)
    }
}
