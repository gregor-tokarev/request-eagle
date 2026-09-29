use gpui_kit::component::{
    button::*,
    menu::{ContextMenuExt, PopupMenuItem},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use tab_ui::Environments;

pub(crate) enum EnvironmentPanelEvent {
    Open(SharedString),
    Rename(SharedString),
    Create,
}

/// The sidebar section that lists global environments.
pub(crate) struct EnvironmentPanel {
    environments: Entity<Environments>,
    selected: Option<usize>,
    pending_delete: Option<SharedString>,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl EventEmitter<EnvironmentPanelEvent> for EnvironmentPanel {}

impl EnvironmentPanel {
    pub(crate) fn new(environments: Entity<Environments>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&environments, |this, environments, cx| {
            let count = environments.read(cx).names().len();
            this.selected = this.selected.filter(|&index| index < count);
            cx.notify();
        });

        Self {
            environments,
            selected: None,
            pending_delete: None,
            focus: cx.focus_handle().tab_stop(true),
            _subscription: subscription,
        }
    }

    #[cfg(test)]
    pub(crate) fn environments(&self) -> Entity<Environments> {
        self.environments.clone()
    }

    fn name(&self, index: usize, cx: &App) -> Option<SharedString> {
        self.environments.read(cx).names().get(index).cloned()
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.selected != Some(index) {
            self.pending_delete = None;
        }

        self.selected = Some(index);
        cx.notify();
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        self.select(index, cx);

        if let Some(name) = self.name(index, cx) {
            cx.emit(EnvironmentPanelEvent::Open(name));
        }
    }

    fn toggle_active(&mut self, name: SharedString, cx: &mut Context<Self>) {
        self.environments.update(cx, |environments, cx| {
            let active = (environments.active() != Some(&name)).then_some(name);
            environments.set_active(active, cx);
        });
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.pending_delete.take() {
            self.environments
                .update(cx, |environments, cx| environments.delete(&name, cx));
        }

        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.environments.read(cx).names().len();
        if count == 0 || event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        let row = self.selected.unwrap_or(0);

        match event.keystroke.key.as_str() {
            "down" => self.select(self.selected.map_or(0, |row| (row + 1).min(count - 1)), cx),
            "up" => self.select(row.saturating_sub(1), cx),
            "home" => self.select(0, cx),
            "end" => self.select(count - 1, cx),
            "enter" if self.pending_delete.is_some() => self.confirm_delete(window, cx),
            "enter" => self.open(row, cx),
            "escape" if self.pending_delete.is_some() => {
                self.pending_delete = None;
                cx.notify();
            }
            _ => return,
        }

        cx.stop_propagation();
    }

    fn delete_prompt(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(("environment-row", index))
            .debug_selector(move || format!("environment-row-{index}"))
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .debug_selector(|| "environment-delete-prompt".into())
                    .size_full()
                    .rounded(cx.theme().radius_tokens().md)
                    .px_2()
                    .gap_1()
                    .bg(cx.theme().sidebar_accent)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .child("Delete environment?"),
                    )
                    .child(
                        Button::new("confirm-environment-delete")
                            .debug_selector(|| "confirm-environment-delete".into())
                            .label("Delete")
                            .tooltip("Confirm deletion (Enter)")
                            .xsmall()
                            .danger()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_delete(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-environment-delete")
                            .debug_selector(|| "cancel-environment-delete".into())
                            .label("Cancel")
                            .tooltip("Cancel deletion (Escape)")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.pending_delete = None;
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn row(&self, index: usize, name: SharedString, cx: &mut Context<Self>) -> AnyElement {
        if self.pending_delete.as_ref() == Some(&name) {
            return self.delete_prompt(index, cx);
        }

        let theme = cx.theme();
        let selected = self.selected == Some(index);
        let active = self.environments.read(cx).active() == Some(&name);
        let view = cx.entity().downgrade();
        let focus = self.focus.clone();

        div()
            .id(("environment-row", index))
            .debug_selector(move || format!("environment-row-{index}"))
            .group("environment-row")
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .size_full()
                    .rounded(theme.radius_tokens().md)
                    .pl_2()
                    .pr_1()
                    .gap_2()
                    .text_sm()
                    .when(selected, |this| {
                        this.bg(theme.tokens.sidebar_accent.background)
                            .text_color(theme.sidebar_accent_foreground)
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(theme.sidebar_accent.opacity(0.55)))
                    })
                    .child(
                        Icon::new(IconName::Globe)
                            .size(rems(0.875))
                            .flex_none()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().text_ellipsis().child(name.clone()))
                    .child(
                        div()
                            .flex_none()
                            .when(!active, |this| {
                                this.invisible()
                                    .group_hover("environment-row", |this| this.visible())
                            })
                            .child(
                                Button::new(("toggle-active-environment", index))
                                    .debug_selector(move || {
                                        format!("toggle-active-environment-{index}")
                                    })
                                    .ghost()
                                    .xsmall()
                                    .icon(
                                        Icon::new(if active {
                                            IconName::CircleCheck
                                        } else {
                                            IconName::Check
                                        })
                                        .size_3p5()
                                        .when(active, |icon| icon.text_color(theme.success)),
                                    )
                                    .accessibility_label(if active {
                                        format!("Stop using {name}")
                                    } else {
                                        format!("Set {name} as active")
                                    })
                                    .tooltip(if active {
                                        "Active environment. Click to stop using it."
                                    } else {
                                        "Set as active environment"
                                    })
                                    .on_click(cx.listener({
                                        let name = name.clone();
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_active(name.clone(), cx);
                                        }
                                    })),
                            ),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.open(index, cx);
            }))
            .capture_any_mouse_down(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.button == MouseButton::Right {
                        window.focus(&this.focus, cx);
                        this.select(index, cx);
                    }
                }),
            )
            .context_menu(move |menu, _, _| {
                let menu = menu.action_context(focus.clone());
                let open_view = view.clone();
                let active_view = view.clone();
                let rename_view = view.clone();
                let delete_view = view.clone();
                let open_name = name.clone();
                let active_name = name.clone();
                let rename_name = name.clone();
                let delete_name = name.clone();

                menu.item(PopupMenuItem::new("Open").on_click(move |_, _, cx| {
                    let _ = open_view.update(cx, |_, cx| {
                        cx.emit(EnvironmentPanelEvent::Open(open_name.clone()))
                    });
                }))
                .item(
                    PopupMenuItem::new(if active {
                        "Stop Using"
                    } else {
                        "Set as Active"
                    })
                    .on_click(move |_, _, cx| {
                        let _ = active_view
                            .update(cx, |this, cx| this.toggle_active(active_name.clone(), cx));
                    }),
                )
                .item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                    let view = rename_view.clone();
                    let name = rename_name.clone();
                    window.defer(cx, move |_, cx| {
                        let _ =
                            view.update(cx, |_, cx| cx.emit(EnvironmentPanelEvent::Rename(name)));
                    });
                }))
                .separator()
                .item(
                    PopupMenuItem::new("Delete Environment").on_click(move |_, window, cx| {
                        let view = delete_view.clone();
                        let name = delete_name.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.pending_delete = Some(name);
                                window.focus(&this.focus, cx);
                                cx.notify();
                            });
                        });
                    }),
                )
            })
            .into_any_element()
    }
}

impl Focusable for EnvironmentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EnvironmentPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let environments = self.environments.read(cx);
        let names = environments.names().to_vec();
        let error = environments.error().map(ToOwned::to_owned);

        v_flex()
            .debug_selector(|| "environments-sidebar".into())
            .size_full()
            .pt_2()
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .px_4()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child("Environments"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(names.len().to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("new-environment")
                            .debug_selector(|| "new-environment".into())
                            .icon(IconName::Plus)
                            .tooltip("New Environment")
                            .ghost()
                            .xsmall()
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(EnvironmentPanelEvent::Create)),
                            ),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                v_flex()
                    .id("environments-list")
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .overflow_y_scrollbar()
                    .when(names.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .p_4()
                                .gap_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No environments yet")
                                .child(
                                    "Create one to share variables, such as a host or token, \
                                     across collections.",
                                ),
                        )
                    })
                    .children(
                        names
                            .into_iter()
                            .enumerate()
                            .map(|(index, name)| self.row(index, name, cx)),
                    ),
            )
    }
}
