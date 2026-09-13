use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    menu::{ContextMenuExt, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{panel::Sidebar, tree::ItemKind};

impl Sidebar {
    pub(super) fn row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[row];
        let item = &self.tree.items[index];
        let branch = item.is_branch();
        let expanded = !self.query.is_empty() || !self.collapsed.contains(&index);
        let selected = self.selected == Some(index);
        let theme = cx.theme();
        let view = cx.entity().downgrade();
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
                .debug_selector(move || format!("collection-row-{index}").into())
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
            .id(("collection-row", index))
            .debug_selector(move || format!("collection-row-{index}").into())
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
                    .when(selected, |this| this.bg(theme.sidebar_accent))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
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
                            .w(px(42.))
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
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.begin_rename(index, window, cx);
                } else {
                    window.focus(&this.focus, cx);
                    this.select_row(row, cx);
                    this.toggle(index, cx);
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
