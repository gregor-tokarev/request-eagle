use std::{collections::HashSet, path::Path};

use collection::{CollectionRegistry, Entry, Method, Request};
use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemKind {
    Collection,
    Folder,
    Request(&'static str),
}

pub(super) struct TreeItem {
    pub label: SharedString,
    pub kind: ItemKind,
    pub depth: usize,
    pub parent: Option<usize>,
    pub end: usize,
    pub request_count: usize,
    search_text: String,
}

impl TreeItem {
    pub fn is_branch(&self) -> bool {
        !matches!(self.kind, ItemKind::Request(_))
    }
}

pub(super) struct CollectionTree {
    pub items: Vec<TreeItem>,
    pub roots: Vec<usize>,
}

impl CollectionTree {
    pub fn new(collections: &CollectionRegistry) -> Self {
        let mut tree = Self {
            items: Vec::new(),
            roots: Vec::new(),
        };

        for collection in collections.collections() {
            let index = tree.items.len();
            let name = path_name(&collection.path);

            tree.roots.push(index);
            tree.items.push(TreeItem {
                search_text: name.to_lowercase(),
                label: name.into(),
                kind: ItemKind::Collection,
                depth: 0,
                parent: None,
                end: 0,
                request_count: 0,
            });
            tree.add_entries(&collection.entries, index);
        }

        tree
    }

    fn add_entries(&mut self, entries: &[Entry], parent: usize) {
        for entry in entries {
            let index = self.items.len();
            let depth = self.items[parent].depth + 1;

            match entry {
                Entry::Directory(folder) => {
                    self.items.push(TreeItem {
                        label: folder.name.clone().into(),
                        search_text: folder.name.to_lowercase(),
                        kind: ItemKind::Folder,
                        depth,
                        parent: Some(parent),
                        end: 0,
                        request_count: 0,
                    });
                    self.add_entries(&folder.entries, index);
                    self.items[parent].request_count += self.items[index].request_count;
                }
                Entry::File(file) => {
                    let Request::Http(request) = &file.request;
                    let method = match request.method {
                        Method::Get => "GET",
                        Method::Post => "POST",
                        Method::Put => "PUT",
                        Method::Delete => "DELETE",
                    };

                    self.items.push(TreeItem {
                        label: file.name.clone().into(),
                        search_text: format!("{method} {} {}", file.name, request.path)
                            .to_lowercase(),
                        kind: ItemKind::Request(method),
                        depth,
                        parent: Some(parent),
                        end: index + 1,
                        request_count: 1,
                    });
                    self.items[parent].request_count += 1;
                }
            }
        }

        self.items[parent].end = self.items.len();
    }

    /// Rebuilt only after interaction, never during a draw or scroll frame.
    pub fn visible_rows(&self, collapsed: &HashSet<usize>, query: &str) -> Vec<usize> {
        if query.is_empty() {
            let mut rows = Vec::new();
            let mut index = 0;

            while index < self.items.len() {
                rows.push(index);
                index = if collapsed.contains(&index) {
                    self.items[index].end
                } else {
                    index + 1
                };
            }

            return rows;
        }

        // Matching a folder includes its descendants. Matching a request keeps
        // its ancestors visible, even when those folders were collapsed.
        let mut included = vec![false; self.items.len()];
        let mut include_until = 0;

        for (index, item) in self.items.iter().enumerate() {
            if index < include_until || item.search_text.contains(query) {
                included[index] = true;
                include_until = include_until.max(item.end);
            }
        }

        for index in (0..self.items.len()).rev() {
            if included[index]
                && let Some(parent) = self.items[index].parent
            {
                included[parent] = true;
            }
        }

        included
            .into_iter()
            .enumerate()
            .filter_map(|(index, included)| included.then_some(index))
            .collect()
    }
}

fn path_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
