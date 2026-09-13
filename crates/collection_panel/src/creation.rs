use std::path::{Path, PathBuf};

use collection::CollectionEditError;
use gpui_kit::{Context, ScrollStrategy, Window};

use super::CollectionPanel;

impl CollectionPanel {
    pub(super) fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.collections.create_collection();
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_request(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.collections.create_request(parent);
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_folder(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.collections.create_folder(parent);
        self.finish_creation(result, window, cx);
    }

    fn finish_creation(
        &mut self,
        result: Result<PathBuf, CollectionEditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(path) => {
                self.rename = None;
                self.pending_delete = None;
                self.error = None;
                // Newly created items must remain visible even with a filter active.
                self.query.clear();
                self.search
                    .update(cx, |search, cx| search.set_value("", window, cx));
                self.collapsed
                    .retain(|&index| !path.starts_with(&self.tree.items[index].path));
                self.rebuild_tree(Some(&path), None, cx);

                if let Some(row) = self.selected_row {
                    self.scroll_handle
                        .scroll_to_item(row, ScrollStrategy::Nearest);
                }
                if let Some(index) = self.selected {
                    self.begin_rename(index, window, cx);
                }
            }
            Err(error) => self.error = Some(format!("Could not create item: {error}")),
        }

        cx.notify();
    }
}
