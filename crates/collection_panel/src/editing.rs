use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};

use super::{
    panel::{CollectionPanel, CollectionPanelEvent},
    tree::CollectionTree,
};

pub(super) struct RenameEditor {
    pub path: PathBuf,
    pub input: Entity<InputState>,
    _subscription: Subscription,
}

impl CollectionPanel {
    pub(super) fn begin_rename(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.tree.items.get(index) else {
            return;
        };
        let path = item.path.clone();
        let name = item.label.clone();
        self.rename = None;
        self.pending_delete = None;
        self.error = None;
        self.selected = Some(index);
        self.selected_row = self.visible.binary_search(&index).ok();

        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).default_value(name);
            input.select_all(window, cx);
            input.focus(window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                InputEvent::Blur => {
                    this.rename = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        self.rename = Some(RenameEditor {
            path,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn on_rename_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            self.rename = None;
            self.error = None;
            window.focus(&self.focus, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = &self.rename else { return };
        let path = rename.path.clone();
        let name = rename.input.read(cx).value();

        match self.collections.rename(&path, &name) {
            Ok(destination) => {
                self.rename = None;
                self.error = None;
                self.rebuild_tree(Some(&destination), Some((&path, &destination)), cx);
                window.focus(&self.focus, cx);
            }
            Err(error) => self.error = Some(format!("Could not rename: {error}")),
        }
        cx.notify();
    }

    pub(super) fn request_delete(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.tree.items.get(index) else {
            return;
        };
        let path = item.path.clone();
        self.rename = None;
        self.error = None;
        if let Ok(row) = self.visible.binary_search(&index) {
            self.select_row(row, cx);
        }
        self.pending_delete = Some(path);
        window.focus(&self.delete_focus, cx);
        cx.notify();
    }

    pub(super) fn cancel_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_delete = None;
        self.error = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn on_delete_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_delete.is_none() || !self.delete_focus.contains_focused(window, cx) {
            return;
        }

        if event.keystroke.key == "escape" {
            self.cancel_delete(window, cx);
            cx.stop_propagation();
        } else if event.keystroke.key == "enter"
            && event.keystroke.modifiers == Modifiers::default()
        {
            self.confirm_delete(window, cx);
            cx.stop_propagation();
        } else if self.delete_focus.is_focused(window)
            && matches!(
                event.keystroke.key.as_str(),
                "backspace" | "space" | "left" | "right"
            )
        {
            // Repeating the delete key must never confirm or collapse the prompt.
            cx.stop_propagation();
        }
    }

    pub(super) fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.pending_delete.clone() else {
            return;
        };
        let row = self.selected_row.unwrap_or(0);

        match self.collections.delete(&path) {
            Ok(()) => {
                self.pending_delete = None;
                self.rename = None;
                self.error = None;
                self.rebuild_tree(None, None, cx);
                if !self.visible.is_empty() {
                    self.select_row(row.min(self.visible.len() - 1), cx);
                }
                window.focus(&self.focus, cx);
            }
            Err(error) => self.error = Some(format!("Could not delete: {error}")),
        }
        cx.notify();
    }

    pub(super) fn rebuild_tree(
        &mut self,
        selected: Option<&Path>,
        renamed: Option<(&Path, &Path)>,
        cx: &mut Context<Self>,
    ) {
        self.rows_task = None;
        let collapsed: HashSet<_> = self
            .collapsed
            .iter()
            .map(|&index| {
                let path = &self.tree.items[index].path;
                if let Some((old, new)) = renamed
                    && let Ok(relative) = path.strip_prefix(old)
                {
                    return new.join(relative);
                }
                path.clone()
            })
            .collect();

        self.tree = Arc::new(CollectionTree::new(&self.collections));
        self.collapsed = self
            .tree
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| collapsed.contains(&item.path).then_some(index))
            .collect();
        self.selected =
            selected.and_then(|path| self.tree.items.iter().position(|item| item.path == path));
        let browsing = Arc::new(self.tree.visible_rows(&self.collapsed, ""));
        self.unfiltered_rows = Some(browsing.clone());
        let rows = if self.query.is_empty() {
            browsing
        } else {
            Arc::new(self.tree.visible_rows(&self.collapsed, &self.query))
        };
        self.apply_rows(rows, false, cx);

        if let Some((previous, destination)) = renamed {
            if let Some(collection) = self
                .collections
                .collections()
                .iter()
                .find(|collection| collection.path == destination)
            {
                cx.emit(CollectionPanelEvent::CollectionRelocated {
                    previous_path: previous.to_path_buf(),
                    path: destination.to_path_buf(),
                    environment_path: collection.local_env().path.clone(),
                });
            }

            for item in &self.tree.items {
                if item.is_branch() {
                    continue;
                }
                let Ok(relative) = item.path.strip_prefix(destination) else {
                    continue;
                };
                let Some(collection) = self
                    .collections
                    .collections()
                    .iter()
                    .find(|collection| item.path.starts_with(&collection.path))
                else {
                    continue;
                };

                let Some(file) = self.collections.file(&item.path) else {
                    continue;
                };

                cx.emit(CollectionPanelEvent::RequestRelocated {
                    id: file.id.clone().into(),
                    previous_path: if relative.as_os_str().is_empty() {
                        previous.to_path_buf()
                    } else {
                        previous.join(relative)
                    },
                    path: item.path.clone(),
                    environment_path: collection.local_env().path.clone(),
                    name: item.label.clone(),
                    folders: item
                        .path
                        .strip_prefix(&collection.path)
                        .ok()
                        .and_then(Path::parent)
                        .map(|path| {
                            path.iter()
                                .map(|part| part.to_string_lossy().into_owned().into())
                                .collect()
                        })
                        .unwrap_or_default(),
                    collection: collection
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                });
            }
        }
    }
}
