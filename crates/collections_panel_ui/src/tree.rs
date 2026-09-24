use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use collection::{CollectionRegistry, Entry, FileEntry, Request};
use gpui_kit::SharedString;

use super::search::SearchIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemKind {
    Collection,
    Folder,
    Request(&'static str),
}

#[derive(Clone)]
pub(super) struct TreeItem {
    pub label: SharedString,
    pub path: PathBuf,
    pub kind: ItemKind,
    pub depth: usize,
    pub parent: Option<usize>,
    pub end: usize,
    pub request_count: usize,
}

impl TreeItem {
    pub fn is_branch(&self) -> bool {
        !matches!(self.kind, ItemKind::Request(_))
    }
}

#[derive(Clone)]
pub(super) struct CollectionTree {
    pub items: Vec<TreeItem>,
    pub roots: Vec<usize>,
    pub search: SearchIndex,
}

impl CollectionTree {
    pub fn new(collections: &CollectionRegistry) -> Self {
        let mut tree = Self {
            items: Vec::new(),
            roots: Vec::new(),
            search: SearchIndex::new([]),
        };
        let mut search_texts = Vec::new();

        for collection in collections.collections() {
            let index = tree.items.len();
            let name = path_name(&collection.path);

            tree.roots.push(index);
            search_texts.push(name.clone());
            tree.items.push(TreeItem {
                label: name.into(),
                path: collection.path.clone(),
                kind: ItemKind::Collection,
                depth: 0,
                parent: None,
                end: 0,
                request_count: 0,
            });
            tree.add_entries(&collection.entries, index, &mut search_texts);
        }

        tree.search = SearchIndex::new(search_texts);

        tree
    }

    pub fn request_changed(&self, file: &FileEntry) -> bool {
        self.items
            .iter()
            .position(|item| item.path == file.path)
            .is_some_and(|index| {
                let Request::Http(request) = &file.request;
                let method = request.method.as_str();

                self.items[index].label.as_ref() != file.name
                    || self.items[index].kind != ItemKind::Request(method)
                    || self.search.document(index)
                        != format!("{method} {} {}", file.name, request.path).to_lowercase()
            })
    }

    pub fn update_request(&mut self, file: &FileEntry) {
        let Some(index) = self.items.iter().position(|item| item.path == file.path) else {
            return;
        };
        let Request::Http(request) = &file.request;
        let method = request.method.as_str();
        self.items[index].label = file.name.clone().into();
        self.items[index].kind = ItemKind::Request(method);
        self.search
            .update(index, format!("{method} {} {}", file.name, request.path));
    }

    fn add_entries(&mut self, entries: &[Entry], parent: usize, search_texts: &mut Vec<String>) {
        for entry in entries {
            let index = self.items.len();
            let depth = self.items[parent].depth + 1;

            match entry {
                Entry::Directory(folder) => {
                    search_texts.push(folder.name.clone());
                    self.items.push(TreeItem {
                        label: folder.name.clone().into(),
                        path: folder.path.clone(),
                        kind: ItemKind::Folder,
                        depth,
                        parent: Some(parent),
                        end: 0,
                        request_count: 0,
                    });
                    self.add_entries(&folder.entries, index, search_texts);
                    self.items[parent].request_count += self.items[index].request_count;
                }
                Entry::File(file) => {
                    let Request::Http(request) = &file.request;
                    let method = request.method.as_str();

                    search_texts.push(format!("{method} {} {}", file.name, request.path));

                    self.items.push(TreeItem {
                        label: file.name.clone().into(),
                        path: file.path.clone(),
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

    /// Nonempty queries use the substring index. The sidebar caches the empty
    /// query's browsing rows until a folder is expanded or collapsed.
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

        self.rows_for_matches(self.search.matching_rows(query))
    }

    fn rows_for_matches(&self, matches: Vec<usize>) -> Vec<usize> {
        let mut rows = Vec::new();
        let mut ancestors = Vec::new();

        // Matches are sorted in tree order. A previously emitted ancestor or
        // subtree never needs visiting again; unrelated branches are untouched.
        for index in matches {
            let emitted_end = rows.last().map_or(0, |&last| last + 1);
            if index < emitted_end {
                continue;
            }

            let mut parent = self.items[index].parent;
            while let Some(index) = parent {
                if index < emitted_end {
                    break;
                }

                ancestors.push(index);
                parent = self.items[index].parent;
            }

            rows.extend(ancestors.drain(..).rev());
            rows.extend(index..self.items[index].end);
        }

        rows
    }
}

fn path_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
