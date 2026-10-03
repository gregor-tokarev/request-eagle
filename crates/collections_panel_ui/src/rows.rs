use std::{cell::Cell, rc::Rc};

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    actions::{DeleteItem, RenameItem},
    dragging::DraggedItem,
    panel::CollectionPanel,
    tree::ItemKind,
};
use collection::MovePlacement;
use request_eagle_theme::{method_label, protocol_icon};

/// How far each level of the tree is indented, in rems.
const INDENT: f32 = 1.;
/// The space before the first level, in rems.
const INSET: f32 = 0.25;
/// The column where branches show their chevron and requests leave a gap,
/// so a request's method starts where a folder's icon does, in rems.
const CHEVRON: f32 = 1.;

type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

impl CollectionPanel {
    pub(super) fn row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[row];
        let item = &self.tree.items[index];

        if item.kind == ItemKind::Empty {
            return self.empty_row(row, cx);
        }

        let delete_label = match item.kind {
            ItemKind::Collection => "Delete collection",
            ItemKind::Folder => "Delete folder",
            ItemKind::Request(_) | ItemKind::Empty => "Delete request",
        };
        let menu = self.row_menu(index, delete_label, cx);
        let branch = item.is_branch();
        let expanded = !self.query.is_empty() || !self.collapsed.contains(&index);
        let selected = self.selected == Some(index);
        let theme = cx.theme();
        let drag_view = cx.entity().downgrade();
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
        let rename = self
            .rename
            .as_ref()
            .filter(|rename| rename.path == item.path);
        let menu_open = self.menu_row.as_ref() == Some(&item.path);

        if self.pending_delete.as_ref() == Some(&item.path) {
            return div()
                .id(ElementId::Path(item.path.clone().into()))
                .debug_selector(move || format!("collection-row-{index}"))
                .track_focus(&self.delete_focus)
                .h_8()
                .w_full()
                .px_2()
                .child(
                    h_flex()
                        .debug_selector(|| "sidebar-delete-prompt".into())
                        .size_full()
                        .rounded(cx.theme().radius_tokens().md)
                        .px_2()
                        .gap_1()
                        .bg(theme.sidebar_accent)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .child(format!("{delete_label}?")),
                        )
                        .child(
                            Button::new("confirm-sidebar-delete")
                                .debug_selector(|| "confirm-sidebar-delete".into())
                                .label("Delete")
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
                                .label("Cancel")
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

        let menu_view = cx.entity().downgrade();
        let menu_path = item.path.clone();

        div()
            .relative()
            .id(ElementId::Path(item.path.clone().into()))
            .debug_selector(move || format!("collection-row-{index}"))
            .group("collection-row")
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .rounded(cx.theme().radius_tokens().md)
                    .pl(rems(INSET + INDENT * item.depth as f32))
                    .pr_1()
                    .gap_1()
                    .text_sm()
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
                    .children(indent_guides(item.depth, cx))
                    .child(
                        div()
                            .flex_none()
                            .w(rems(CHEVRON))
                            .flex()
                            .justify_center()
                            .text_color(theme.muted_foreground)
                            .when(branch, |this| {
                                this.child(
                                    Icon::new(if expanded {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size_3(),
                                )
                            }),
                    )
                    .map(|this| match item.kind {
                        ItemKind::Request(method) => {
                            this.child(div().flex_none().mr_1().child(method_label(method, cx)))
                        }
                        ItemKind::Folder => this.child(
                            Icon::new(if expanded {
                                IconName::FolderOpen
                            } else {
                                IconName::FolderClosed
                            })
                            .size(rems(0.875))
                            .flex_none()
                            .mr_1()
                            .text_color(theme.muted_foreground),
                        ),
                        // Collections are the roots of the tree, so their
                        // name is enough.
                        ItemKind::Collection | ItemKind::Empty => this,
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
                    .when(rename.is_none(), |this| {
                        // Shown while the row is hovered, selected or its
                        // menu is open, as the context menu's twin.
                        this.child(
                            div()
                                .id(("collection-row-menu-slot", index))
                                .flex_none()
                                .when(!selected && !menu_open, |this| {
                                    this.invisible()
                                        .group_hover("collection-row", |this| this.visible())
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    Button::new(("collection-row-menu", index))
                                        .debug_selector(move || {
                                            format!("collection-row-menu-{index}")
                                        })
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Ellipsis)
                                        .accessibility_label(format!(
                                            "More actions for {}",
                                            item.label
                                        ))
                                        .dropdown_menu_with_anchor(Anchor::TopRight, {
                                            let menu = menu.clone();
                                            move |popup, window, cx| menu(popup, window, cx)
                                        })
                                        .on_open_change(move |open, _, cx| {
                                            let open = *open;
                                            let path = menu_path.clone();
                                            let _ = menu_view.update(cx, |this, cx| {
                                                if open {
                                                    this.menu_row = Some(path);
                                                } else if this.menu_row.as_ref() == Some(&path) {
                                                    this.menu_row = None;
                                                }
                                                cx.notify();
                                            });
                                        }),
                                ),
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
                            .h(rems(0.125))
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
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // Quick presses after a branch toggle toggle that branch again,
                // even if collapsing it moved another row under the pointer.
                let index = match &this.clicked_branch {
                    Some(path) if event.click_count() > 1 => this.tree.index_of(path),
                    _ => Some(index),
                };
                let Some(index) = index else {
                    return;
                };
                let Ok(row) = this.visible.binary_search(&index) else {
                    return;
                };

                window.focus(&this.focus, cx);
                this.select_row(row, cx);

                // Branches expand or collapse; collections and requests open a tab.
                if this.tree.items[index].is_branch() {
                    this.toggle(index, cx);
                    this.clicked_branch = Some(this.tree.items[index].path.clone());
                }
                this.open(index, cx);
            }))
            .capture_any_mouse_down(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.button == MouseButton::Right {
                        window.focus(&this.focus, cx);
                        this.select_row(row, cx);
                    }
                }),
            )
            .context_menu(move |popup, window, cx| menu(popup, window, cx))
            .into_any_element()
    }

    /// Stands in for the contents of an empty collection or folder, with a
    /// shortcut to its first request. Dropping an item on it moves the item
    /// into the branch.
    fn empty_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[row];
        let item = &self.tree.items[index];
        let parent = item.parent.map(|parent| &self.tree.items[parent]);
        let collection = parent.is_some_and(|parent| parent.kind == ItemKind::Collection);
        let parent_path = item.path.clone();
        let theme = cx.theme();
        let dropping = self
            .drop_target
            .is_some_and(|(target, _)| target == index && cx.has_active_drag());

        div()
            .id(("collection-empty", index))
            .debug_selector(move || format!("collection-row-{index}"))
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .rounded(theme.radius_tokens().md)
                    .pl(rems(INSET + INDENT * item.depth as f32 + CHEVRON))
                    .pr_1()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .when(dropping, |this| this.bg(theme.info.opacity(0.25)))
                    .children(indent_guides(item.depth, cx))
                    // In a narrow sidebar the message gives way, so the
                    // shortcut stays in reach.
                    .child(div().min_w_0().ml_1().truncate().child(if collection {
                        "This collection is empty."
                    } else {
                        "This folder is empty."
                    }))
                    .child(
                        div().flex_none().child(
                            Button::new(("collection-empty-add", index))
                                .debug_selector(move || format!("collection-empty-add-{index}"))
                                .link()
                                .xsmall()
                                .label("Add a request")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.create_request(&parent_path, window, cx)
                                })),
                        ),
                    ),
            )
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DraggedItem>, _, cx| {
                    this.drag_over_row(index, event, cx);
                }),
            )
            .on_drop(cx.listener(move |this, drag: &DraggedItem, window, cx| {
                if drag.owner != cx.entity_id() {
                    return;
                }
                if let Some(placement) =
                    this.drop_placement(index, drag, Bounds::default(), window.mouse_position())
                {
                    this.move_item(&drag.path, index, placement, window, cx);
                }
            }))
            .into_any_element()
    }

    /// What right-clicking a row or its "…" button offers.
    fn row_menu(
        &self,
        index: usize,
        delete_label: &'static str,
        cx: &mut Context<Self>,
    ) -> MenuBuilder {
        let item = &self.tree.items[index];
        let branch = item.is_branch();
        let view = cx.entity().downgrade();
        let focus = self.focus.clone();
        let path = item.path.clone();
        let run_label = if item.kind == ItemKind::Collection {
            "Run collection"
        } else {
            "Run folder"
        };

        Rc::new(move |menu, _, cx| {
            // Resolve shortcuts in the tree's key context. Clicks still run
            // the item handlers, which target the clicked row's path.
            let menu = menu.action_context(focus.clone());
            let rename_view = view.clone();
            let delete_view = view.clone();
            let path = path.clone();
            let rename_path = path.clone();
            let delete_path = path.clone();
            let copied_path = path.to_string_lossy().into_owned();

            let menu = if branch {
                let run_view = view.clone();
                let run_path = path.clone();
                let request_view = view.clone();
                let request_parent = path.clone();
                let grpc_view = view.clone();
                let grpc_parent = path.clone();
                let websocket_view = view.clone();
                let websocket_parent = path.clone();
                let folder_view = view.clone();
                let folder_parent = path.clone();

                // New requests show their protocol icons, as in the tab
                // bar's new tab menu, and folders their tree icon.
                menu.item(
                    PopupMenuItem::new(run_label)
                        .icon(Icon::new(IconName::Play).text_color(cx.theme().muted_foreground))
                        .on_click(move |_, window, cx| {
                            let view = run_view.clone();
                            let path = run_path.clone();
                            window.defer(cx, move |_, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    if let Some(event) = this
                                        .tree
                                        .index_of(&path)
                                        .and_then(|index| this.run_event(index))
                                    {
                                        cx.emit(event);
                                    }
                                });
                            });
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("New Request")
                        .icon(protocol_icon("HTTP", cx))
                        .on_click(move |_, window, cx| {
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
                    PopupMenuItem::new("New gRPC Request")
                        .icon(protocol_icon("gRPC", cx))
                        .on_click(move |_, window, cx| {
                            let view = grpc_view.clone();
                            let parent = grpc_parent.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.create_grpc_request(&parent, window, cx)
                                });
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("New WebSocket")
                        .icon(protocol_icon("WS", cx))
                        .on_click(move |_, window, cx| {
                            let view = websocket_view.clone();
                            let parent = websocket_parent.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.create_websocket(&parent, window, cx)
                                });
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("New Folder")
                        .icon(
                            Icon::new(IconName::FolderClosed)
                                .text_color(cx.theme().muted_foreground),
                        )
                        .on_click(move |_, window, cx| {
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

            menu.item(
                PopupMenuItem::new("Rename")
                    .action(Box::new(RenameItem))
                    .on_click(move |_, window, cx| {
                        let view = rename_view.clone();
                        let path = rename_path.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                if let Some(index) = this.tree.index_of(&path) {
                                    this.begin_rename(index, window, cx);
                                }
                            });
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("Open in Finder")
                    .on_click(move |_, _, cx| cx.reveal_path(&path)),
            )
            .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied_path.clone()));
            }))
            .separator()
            .item(
                PopupMenuItem::new(delete_label)
                    .action(Box::new(DeleteItem))
                    .on_click(move |_, window, cx| {
                        let view = delete_view.clone();
                        let path = delete_path.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                if let Some(index) = this.tree.index_of(&path) {
                                    this.request_delete(index, window, cx);
                                }
                            });
                        });
                    }),
            )
        })
    }
}

/// A line under the chevron of each branch that holds the row, so nesting
/// reads at a glance.
fn indent_guides(depth: usize, cx: &App) -> impl Iterator<Item = AnyElement> + use<> {
    let color = cx.theme().sidebar_border;

    (0..depth).map(move |level| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(rems(INSET + INDENT * level as f32 + CHEVRON / 2.))
            .w(px(1.))
            .bg(color)
            .into_any_element()
    })
}
