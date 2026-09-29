use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, SharedString, Task};

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

/// Lookups for callers outside the sidebar, such as the command palette.
/// Queries are matched case-insensitively; an empty query matches every
/// collection, but no requests. Each returns at most `limit` matches, in tree
/// order.
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

    /// Open a collection or request in a tab, as if its row were activated.
    pub fn open_at(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(index) = self.tree.items.iter().position(|item| item.path == path) {
            self.open(index, cx);
        }
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
