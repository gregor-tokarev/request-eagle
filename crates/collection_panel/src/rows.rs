use std::{cell::Cell, rc::Rc};

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    menu::{ContextMenuExt, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    dragging::DraggedItem,
    panel::{CollectionPanel, CollectionPanelEvent},
    tree::ItemKind,
};
use collection::MovePlacement;

impl CollectionPanel {
    pub(super) fn row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[row];
        let item = &self.tree.items[index];
        let branch = item.is_branch();
        let expanded = !self.query.is_empty() || !self.collapsed.contains(&index);
        let selected = self.selected == Some(index);
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        let drag_view = view.clone();
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let row_bounds = bounds.clone();
        let drop_position = self
            .drop_target
            .filter(|(target, _)| *target == index && cx.has_active_drag())
            .map(|(_, placement)| placement);
        let drag = DraggedItem {
            path: item.path.clone(),
            label: item.label.clone(),
            owner: cx.entity_id(),
        };
        let path = item.path.clone();
        let rename = self
            .rename
            .as_ref()
            .filter(|rename| rename.path == item.path);
        let delete_label = match item.kind {
            ItemKind::Collection => "Delete collection",
            ItemKind::Folder => "Delete folder",
            ItemKind::Request(_) => "Delete request",
        };

        if self.pending_delete.as_ref() == Some(&item.path) {
            return div()
                .id(("collection-row", index))
                .debug_selector(move || format!("collection-row-{index}"))
                .track_focus(&self.delete_focus)
                .h(px(30.))
                .w_full()
                .px_2()
                .child(
                    h_flex()
                        .debug_selector(|| "sidebar-delete-prompt".into())
                        .size_full()
                        .rounded_md()
                        .px_2()
                        .gap_1()
                        .bg(theme.sidebar_accent)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .child(format!("{delete_label}?")),
                        )
                        .child(
                            Button::new("confirm-sidebar-delete")
                                .debug_selector(|| "confirm-sidebar-delete".into())
                                .label("Yes")
                                .tooltip("Confirm deletion (Enter)")
                                .xsmall()
                                .danger()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_delete(window, cx)
                                })),
                        )
                        .child(
                            Button::new("cancel-sidebar-delete")
                                .debug_selector(|| "cancel-sidebar-delete".into())
                                .label("No")
                                .tooltip("Cancel deletion (Escape)")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_delete(window, cx)
                                })),
                        ),
                )
                .into_any_element();
        }

        div()
            .relative()
            .id(("collection-row", index))
            .debug_selector(move || format!("collection-row-{index}"))
            .h(px(30.))
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .size_full()
                    .rounded_md()
                    .pl(px(6. + item.depth as f32 * 14.))
                    .pr_2()
                    .gap_1p5()
                    .text_size(px(13.))
                    .when(selected, |this| {
                        this.bg(theme.tokens.sidebar_accent.background)
                            .text_color(theme.sidebar_accent_foreground)
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
                    })
                    .when(drop_position == Some(MovePlacement::Inside), |this| {
                        this.bg(theme.info.opacity(0.25))
                    })
                    .child(if branch {
                        h_flex()
                            .gap_1p5()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(12.)),
                            )
                            .child(Icon::new(IconName::Folder).size(px(14.)))
                            .into_any_element()
                    } else {
                        let ItemKind::Request(method) = item.kind else {
                            unreachable!()
                        };
                        let color = match method {
                            "GET" => theme.success,
                            "POST" => theme.warning,
                            "PUT" => theme.info,
                            _ => theme.danger,
                        };

                        div()
                            .flex_none()
                            .text_size(px(9.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(method)
                            .into_any_element()
                    })
                    .child(if let Some(rename) = rename {
                        div()
                            .id("sidebar-rename-editor")
                            .debug_selector(|| "sidebar-rename-editor".into())
                            .flex_1()
                            .min_w_0()
                            .capture_key_down(cx.listener(Self::on_rename_key_down))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(Input::new(&rename.input).small())
                            .into_any_element()
                    } else {
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .when(item.kind == ItemKind::Collection, |this| {
                                this.font_weight(FontWeight::MEDIUM)
                            })
                            .child(item.label.clone())
                            .into_any_element()
                    })
                    .when(branch, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(theme.muted_foreground)
                                .child(item.request_count.to_string()),
                        )
                    }),
            )
            .when(
                matches!(
                    drop_position,
                    Some(MovePlacement::Before | MovePlacement::After)
                ),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_2()
                            .right_2()
                            .h(px(2.))
                            .bg(theme.info)
                            .when(drop_position == Some(MovePlacement::Before), |this| {
                                this.top_0()
                            })
                            .when(drop_position == Some(MovePlacement::After), |this| {
                                this.bottom_0()
                            }),
                    )
                },
            )
            .when(
                rename.is_none() && item.kind != ItemKind::Collection,
                |this| {
                    this.on_drag(drag, move |drag, _, window, cx| {
                        let _ = drag_view.update(cx, |this, cx| {
                            this.pending_delete = None;
                            this.drop_target = None;
                            this.select_row(row, cx);
                            window.focus(&this.focus, cx);
                        });
                        cx.new(|_| drag.clone())
                    })
                },
            )
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DraggedItem>, _, cx| {
                    this.drag_over_row(index, event, cx);
                }),
            )
            .child(
                canvas(move |bounds, _, _| row_bounds.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .on_drop(cx.listener(move |this, drag: &DraggedItem, window, cx| {
                if drag.owner != cx.entity_id() {
                    return;
                }
                // A quick drag may arrive without a separate hover event.
                if let Some(placement) =
                    this.drop_placement(index, drag, bounds.get(), window.mouse_position())
                {
                    this.move_item(&drag.path, index, placement, window, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.select_row(row, cx);

                if branch {
                    this.toggle(index, cx);
                } else if let Some(file) = this.collections.file(&this.tree.items[index].path) {
                    let item = &this.tree.items[index];
                    let mut root = index;

                    while let Some(parent) = this.tree.items[root].parent {
                        root = parent;
                    }

                    cx.emit(CollectionPanelEvent::OpenRequest {
                        id: file.id.clone().into(),
                        path: item.path.clone(),
                        name: item.label.clone(),
                        collection: this.tree.items[root].label.clone(),
                        request: file.request.clone(),
                    });
                }
            }))
            .context_menu(move |menu, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    window.focus(&this.focus, cx);
                    this.select_row(row, cx);
                });
                let rename_view = view.clone();
                let delete_view = view.clone();
                let path = path.clone();
                let copied_path = path.to_string_lossy().into_owned();

                let menu = if branch {
                    let request_view = view.clone();
                    let request_parent = path.clone();
                    let folder_view = view.clone();
                    let folder_parent = path.clone();

                    menu.item(
                        PopupMenuItem::new("New Request").on_click(move |_, window, cx| {
                            let view = request_view.clone();
                            let parent = request_parent.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.create_request(&parent, window, cx)
                                });
                            });
                        }),
                    )
                    .item(
                        PopupMenuItem::new("New Folder").on_click(move |_, window, cx| {
                            let view = folder_view.clone();
                            let parent = folder_parent.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = view
                                    .update(cx, |this, cx| this.create_folder(&parent, window, cx));
                            });
                        }),
                    )
                    .separator()
                } else {
                    menu
                };

                menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                    let view = rename_view.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = view.update(cx, |this, cx| this.begin_rename(index, window, cx));
                    });
                }))
                .item(
                    PopupMenuItem::new("Open in Finder")
                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                )
                .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied_path.clone()));
                }))
                .separator()
                .item(
                    PopupMenuItem::new(delete_label).on_click(move |_, window, cx| {
                        let view = delete_view.clone();
                        window.defer(cx, move |window, cx| {
                            let _ =
                                view.update(cx, |this, cx| this.request_delete(index, window, cx));
                        });
                    }),
                )
            })
            .into_any_element()
    }
}
