use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::{Context, Focusable as _, SharedString, Window};

use super::{CollectionPanel, tree::ItemKind};

pub struct CollectionMatch {
    pub path: PathBuf,
    pub name: SharedString,
    pub request_count: usize,
}

pub struct RequestMatch {
    pub path: PathBuf,
    pub name: SharedString,
    pub method: &'static str,
    /// The collection and folders that contain the request.
    pub location: SharedString,
}

/// A collection's `environment.toml`, named after its collection.
pub struct EnvironmentMatch {
    pub path: PathBuf,
    pub name: SharedString,
    pub variable_count: usize,
}

/// Lookups for callers outside the sidebar, such as the command palette.
/// Queries are matched case-insensitively; an empty query matches every
/// collection and environment, but no requests.
impl CollectionPanel {
    pub fn find_collections(&self, query: &str) -> Vec<CollectionMatch> {
        let query = query.to_lowercase();

        self.tree
            .roots
            .iter()
            .map(|&index| &self.tree.items[index])
            .filter(|item| item.label.to_lowercase().contains(&query))
            .map(|item| CollectionMatch {
                path: item.path.clone(),
                name: item.label.clone(),
                request_count: item.request_count,
            })
            .collect()
    }

    /// Match request names, methods and URLs like the sidebar filter, in tree
    /// order. The sidebar's index keeps this fast in very large collections.
    pub fn find_requests(&self, query: &str, limit: usize) -> Vec<RequestMatch> {
        self.tree
            .search
            .matching_rows(query)
            .into_iter()
            .filter_map(|index| {
                let item = &self.tree.items[index];
                let ItemKind::Request(method) = item.kind else {
                    return None;
                };

                let mut folders = Vec::new();
                let mut parent = item.parent;
                while let Some(index) = parent {
                    folders.push(self.tree.items[index].label.as_ref());
                    parent = self.tree.items[index].parent;
                }
                folders.reverse();

                Some(RequestMatch {
                    path: item.path.clone(),
                    name: item.label.clone(),
                    method,
                    location: folders.join(" › ").into(),
                })
            })
            .take(limit)
            .collect()
    }

    /// Environments whose collection or variable names contain the query.
    /// Collections without environment variables or a file are skipped.
    pub fn find_environments(&self, query: &str) -> Vec<EnvironmentMatch> {
        let query = query.to_lowercase();

        self.collections
            .collections()
            .iter()
            .filter_map(|collection| {
                let environment = collection.local_env();
                if environment.entries.is_empty() && !environment.path.is_file() {
                    return None;
                }

                let name = collection
                    .path
                    .file_name()
                    .unwrap_or(collection.path.as_os_str())
                    .to_string_lossy()
                    .into_owned();

                let matches = name.to_lowercase().contains(&query)
                    || environment
                        .entries
                        .keys()
                        .any(|variable| variable.to_lowercase().contains(&query));

                matches.then(|| EnvironmentMatch {
                    path: environment.path.clone(),
                    name: name.into(),
                    variable_count: environment.entries.len(),
                })
            })
            .collect()
    }

    /// Open a request in a tab, as if its row were activated.
    pub fn open_request_at(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(index) = self.tree.items.iter().position(|item| item.path == path) {
            self.open_request(index, cx);
        }
    }

    /// Clear the filter, expand the row's ancestors, then select and focus it.
    pub fn reveal(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tree.items.iter().position(|item| item.path == path) else {
            return;
        };

        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));

        let mut parent = self.tree.items[index].parent;
        while let Some(ancestor) = parent {
            self.collapsed.remove(&ancestor);
            parent = self.tree.items[ancestor].parent;
        }

        self.rows_task = None;
        let rows = Arc::new(self.tree.visible_rows(&self.collapsed, ""));
        self.unfiltered_rows = Some(rows.clone());
        self.apply_rows(rows, false, cx);

        if let Ok(row) = self.visible.binary_search(&index) {
            self.select_row(row, cx);
        }

        window.focus(&self.focus_handle(cx), cx);
    }
}
