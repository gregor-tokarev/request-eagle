use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::CollectionEditError;
use gpui_kit::{
    component::{
        WindowExt as _,
        input::{InputEvent, InputState},
    },
    *,
};

use super::{
    delete_warning::{AffectedFlow, ConfirmationCheck, TargetSnapshot, recheck_confirmation},
    panel::CollectionPanel,
    tree::{CollectionTree, ItemKind},
};

pub(super) struct RenameEditor {
    pub path: PathBuf,
    pub input: Entity<InputState>,
    _subscription: Subscription,
}

impl CollectionPanel {
    pub fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .collections
            .update(cx, |collections, cx| collections.create_collection(cx));
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_request(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .collections
            .update(cx, |collections, cx| collections.create_request(parent, cx));
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_grpc_request(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.collections.update(cx, |collections, cx| {
            collections.create_request_with(
                parent,
                "New gRPC Request",
                request::GrpcRequest::default().into(),
                cx,
            )
        });
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_websocket(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.collections.update(cx, |collections, cx| {
            collections.create_request_with(
                parent,
                "New WebSocket",
                request::WebSocketRequest::default().into(),
                cx,
            )
        });
        self.finish_creation(result, window, cx);
    }

    pub(super) fn create_folder(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .collections
            .update(cx, |collections, cx| collections.create_folder(parent, cx));
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
                self.clear_pending_delete();
                self.error = None;
                self.reveal(&path, None, window, cx);

                if let Some(row) = self.selected_row() {
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

    pub(super) fn begin_rename(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .tree
            .items
            .get(index)
            .filter(|item| item.kind != ItemKind::Empty)
        else {
            return;
        };
        let path = item.path.clone();
        let name = item.label.clone();
        self.rename = None;
        self.clear_pending_delete();
        self.error = None;
        self.selected = Some(index);

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

        match self
            .collections
            .update(cx, |collections, cx| collections.rename(&path, &name, cx))
        {
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
        let Some(item) = self
            .tree
            .items
            .get(index)
            .filter(|item| item.kind != ItemKind::Empty)
        else {
            return;
        };
        let path = item.path.clone();
        let Some(target) = TargetSnapshot::capture(self.collections.read(cx), &self.tree, &path)
        else {
            return;
        };

        self.rename = None;
        self.error = None;
        if let Ok(row) = self.visible.binary_search(&index) {
            self.select_row(row, cx);
        }

        self.finish_request_delete(target, window, cx);
    }

    fn finish_request_delete(
        &mut self,
        original: TargetSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(wait) = (self.wait_for_refreshes)(cx) {
            self.clear_pending_delete();
            self.delete_check = Some(cx.spawn_in(window, async move |this, cx| {
                wait.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.delete_check = None;
                    if window.has_active_dialog(cx) {
                        cx.notify();
                        return;
                    }
                    this.finish_request_delete(original, window, cx);
                });
            }));
            cx.notify();
            return;
        }

        let current =
            TargetSnapshot::capture(self.collections.read(cx), &self.tree, &original.path);
        match current {
            None => {
                self.clear_pending_delete();
                self.error = Some("This item is no longer saved.".to_owned());
                window.focus(&self.focus, cx);
                cx.notify();
                return;
            }
            Some(current) if current != original => {
                self.clear_pending_delete();
                self.error =
                    Some("This item changed while refreshing. Try deleting it again.".to_owned());
                window.focus(&self.focus, cx);
                cx.notify();
                return;
            }
            Some(_) => {}
        }

        let affected = (self.affected_flows)(&original.path, cx);
        if !affected.is_empty() {
            self.clear_pending_delete();
            self.open_delete_warning(original, affected, window, cx);
            cx.notify();
            return;
        }

        self.delete_check = None;
        self.pending_delete = Some(original);
        window.focus(&self.delete_focus, cx);
        cx.notify();
    }

    pub(super) fn recheck_delete(
        &self,
        previous_target: &TargetSnapshot,
        previous_affected: &[AffectedFlow],
        cx: &App,
    ) -> ConfirmationCheck {
        let current_target =
            TargetSnapshot::capture(self.collections.read(cx), &self.tree, &previous_target.path);
        let current_affected = (self.affected_flows)(&previous_target.path, cx);

        recheck_confirmation(
            previous_target,
            previous_affected,
            current_target,
            current_affected,
        )
    }

    pub(super) fn cancel_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_pending_delete();
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
        if self.delete_check.is_some() {
            return;
        }

        let Some(pending) = self.pending_delete.clone() else {
            return;
        };

        if let Some(wait) = (self.wait_for_refreshes)(cx) {
            self.delete_check = Some(cx.spawn_in(window, async move |this, cx| {
                wait.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.delete_check = None;
                    if window.has_active_dialog(cx) {
                        this.clear_pending_delete();
                        cx.notify();
                        return;
                    }
                    this.confirm_delete(window, cx);
                });
            }));
            cx.notify();
            return;
        }

        match self.recheck_delete(&pending, &[], cx) {
            ConfirmationCheck::Proceed => {}
            ConfirmationCheck::Missing => {
                self.clear_pending_delete();
                self.error = Some("This item is no longer saved.".to_owned());
                window.focus(&self.focus, cx);
                cx.notify();
                return;
            }
            ConfirmationCheck::Changed {
                target, affected, ..
            } => {
                if !affected.is_empty() {
                    self.clear_pending_delete();
                    self.error = None;
                    self.open_delete_warning(target, affected, window, cx);
                    cx.notify();
                    return;
                }

                self.pending_delete = Some(target);
                self.error = Some("This item changed before deletion. Confirm again.".to_owned());
                cx.notify();
                return;
            }
        }

        if let Err(error) = self.delete_path(&pending.path, window, cx) {
            self.error = Some(format!("Could not delete: {error}"));
            cx.notify();
        }
    }

    pub(super) fn delete_path(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), CollectionEditError> {
        let row = self.selected_row().unwrap_or(0);

        // A linked collection's folder stays; only the app forgets it.
        self.collections.update(cx, |collections, cx| {
            if collections.registry().is_linked(path) {
                collections.unlink(path, cx)
            } else {
                collections.delete(path, cx)
            }
        })?;

        self.clear_pending_delete();
        self.rename = None;
        self.error = None;
        self.rebuild_tree(None, None, cx);
        if !self.visible.is_empty() {
            self.select_row(row.min(self.visible.len() - 1), cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
        Ok(())
    }

    /// Clear the filter and expand the ancestors of a created or moved item so
    /// that it is visible, then select it. The item keeps its own collapsed state.
    pub(super) fn reveal(
        &mut self,
        path: &Path,
        renamed: Option<(&Path, &Path)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.collapsed.retain(|&index| {
            let item = &self.tree.items[index].path;
            item == path || !path.starts_with(item)
        });
        self.rebuild_tree(Some(path), renamed, cx);
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

        let collections = self.collections.read(cx);
        self.tree = Arc::new(CollectionTree::new(collections.registry()));
        self.revision = collections.revision();
        self.collapsed = self.tree.branches_at(&collapsed);
        self.selected = selected.and_then(|path| self.tree.index_of(path));
        let browsing = Arc::new(self.tree.visible_rows(&self.collapsed, ""));
        self.unfiltered_rows = Some(browsing.clone());
        let rows = if self.query.is_empty() {
            browsing
        } else {
            Arc::new(self.tree.visible_rows(&self.collapsed, &self.query))
        };
        self.apply_rows(rows, false, cx);
    }
}
