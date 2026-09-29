use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::{App, Context, Focusable as _, SharedString, Task, Window};

use super::{
    CollectionPanel,
    tree::{CollectionTree, ItemKind},
};

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
/// collection and environment, but no requests. Each returns at most `limit`
/// matches, in tree order.
impl CollectionPanel {
    pub fn find_collections(&self, query: &str, limit: usize) -> Vec<CollectionMatch> {
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
            .take(limit)
            .collect()
    }

    /// Match request names, methods and URLs like the sidebar filter, in tree
    /// order. A broad query can match most of a very large collection, so
    /// search off the UI thread like the sidebar; drop the task to cancel it.
    pub fn find_requests(&self, query: &str, limit: usize, cx: &App) -> Task<Vec<RequestMatch>> {
        let tree = self.tree.clone();
        let query = query.to_owned();

        cx.background_executor()
            .spawn(async move { request_matches(&tree, &query, limit) })
    }

    /// Environments whose collection or variable names contain the query.
    /// Uses the variables loaded with the collections, so it never reads the
    /// disk; collections without environment variables are skipped.
    pub fn find_environments(&self, query: &str, limit: usize) -> Vec<EnvironmentMatch> {
        let query = query.to_lowercase();

        self.collections
            .collections()
            .iter()
            .filter_map(|collection| {
                let environment = collection.local_env();
                if environment.entries.is_empty() {
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
            .take(limit)
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

        // Expand the row itself too, so a revealed collection shows its requests.
        let mut row = Some(index);
        while let Some(expanded) = row {
            self.collapsed.remove(&expanded);
            row = self.tree.items[expanded].parent;
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

fn request_matches(tree: &CollectionTree, query: &str, limit: usize) -> Vec<RequestMatch> {
    tree.search
        .matching_rows(query)
        .into_iter()
        .filter_map(|index| {
            let item = &tree.items[index];
            let ItemKind::Request(method) = item.kind else {
                return None;
            };

            let mut folders = Vec::new();
            let mut parent = item.parent;
            while let Some(index) = parent {
                folders.push(tree.items[index].label.as_ref());
                parent = tree.items[index].parent;
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
