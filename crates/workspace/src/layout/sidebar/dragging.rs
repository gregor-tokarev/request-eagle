use std::path::{Path, PathBuf};

use collection::MovePlacement;
use gpui_kit::{component::*, *};

use super::{Sidebar, tree::ItemKind};

#[derive(Clone)]
pub(super) struct DraggedItem {
    pub path: PathBuf,
    pub label: SharedString,
    pub owner: EntityId,
}

impl Render for DraggedItem {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(cx.theme().sidebar_accent)
            .text_color(cx.theme().sidebar_foreground)
            .text_size(px(13.))
            .child(self.label.clone())
    }
}

impl Sidebar {
    pub(super) fn drag_over_row(
        &mut self,
        index: usize,
        event: &DragMoveEvent<DraggedItem>,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        if drag.owner != cx.entity_id() {
            return;
        }
        if !event.bounds.contains(&event.event.position) {
            if self.drop_target.is_some_and(|(target, _)| target == index) {
                self.drop_target = None;
                cx.notify();
            }
            return;
        }

        let target = self
            .drop_placement(index, drag, event.bounds, event.event.position)
            .map(|placement| (index, placement));
        if self.drop_target != target {
            self.drop_target = target;
            cx.notify();
        }
    }

    pub(super) fn drop_placement(
        &self,
        index: usize,
        drag: &DraggedItem,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
    ) -> Option<MovePlacement> {
        let item = &self.tree.items[index];
        let y = position.y - bounds.top();
        let placement = if item.kind == ItemKind::Collection {
            MovePlacement::Inside
        } else if item.is_branch() {
            if y < px(7.) {
                MovePlacement::Before
            } else if y > bounds.size.height - px(7.) {
                MovePlacement::After
            } else {
                MovePlacement::Inside
            }
        } else if y < bounds.size.height / 2. {
            MovePlacement::Before
        } else {
            MovePlacement::After
        };
        let parent = if placement == MovePlacement::Inside {
            item.path.as_path()
        } else {
            item.path.parent().unwrap()
        };
        (drag.path != item.path && !parent.starts_with(&drag.path)).then_some(placement)
    }

    pub(super) fn move_item(
        &mut self,
        source: &Path,
        target: usize,
        placement: MovePlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drop_target = None;
        let target = self.tree.items[target].path.clone();
        match self.collections.move_entry(source, &target, placement) {
            Ok(destination) => {
                self.rename = None;
                self.pending_delete = None;
                self.error = None;
                self.query.clear();
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                // Preserve the moved subtree's collapsed state while revealing its new parent.
                self.collapsed.retain(|&index| {
                    let path = &self.tree.items[index].path;
                    path == source || !destination.starts_with(path)
                });
                self.rebuild_tree(Some(&destination), Some((source, &destination)), cx);
                if let Some(row) = self.selected_row {
                    self.select_row(row, cx);
                }
                window.focus(&self.focus, cx);
            }
            Err(error) => self.error = Some(format!("Could not move item: {error}")),
        }
        cx.notify();
    }
}
