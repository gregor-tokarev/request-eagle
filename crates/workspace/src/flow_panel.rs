use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use flow::{Flow, FlowLibrary, FlowLibraryError, SavedFlow};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem},
    scroll::Scrollbar,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

pub(crate) enum FlowPanelEvent {
    Open(SavedFlow),
    Renamed { path: PathBuf, name: SharedString },
    Deleted { path: PathBuf },
}

/// A flow whose name is being edited in its row.
struct FlowRename {
    path: PathBuf,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// The sidebar section that lists saved flows. Flows are saved apart from
/// collections, like Postman's.
pub(crate) struct FlowPanel {
    library: FlowLibrary,
    selected: Option<PathBuf>,
    rename: Option<FlowRename>,
    pending_delete: Option<PathBuf>,
    /// The row whose "…" menu is open, which keeps its button shown.
    menu_row: Option<PathBuf>,
    error: Option<String>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

impl EventEmitter<FlowPanelEvent> for FlowPanel {}

impl FlowPanel {
    pub(crate) fn new(library: FlowLibrary, cx: &mut Context<Self>) -> Self {
        Self {
            library,
            selected: None,
            rename: None,
            pending_delete: None,
            menu_row: None,
            error: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.library.flows().len()
    }

    pub(crate) fn get(&self, path: &Path) -> Option<&SavedFlow> {
        self.library.get(path)
    }

    pub(crate) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
            || self
                .rename
                .as_ref()
                .is_some_and(|rename| rename.input.focus_handle(cx).is_focused(window))
    }

    /// Save a new flow with a Start block and select it. Returns its path.
    pub(crate) fn create(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        self.rename = None;
        self.pending_delete = None;

        match self.library.create("New Flow", Flow::starter()) {
            Ok(path) => {
                self.error = None;
                self.select(path.clone(), cx);
                Some(path)
            }
            Err(error) => {
                self.error = Some(format!("Could not create flow: {error}"));
                cx.notify();
                None
            }
        }
    }

    /// Save a flow tab's blocks and connections.
    pub(crate) fn save(
        &mut self,
        path: &Path,
        expected_id: &str,
        flow: Flow,
    ) -> Result<(), FlowLibraryError> {
        self.library.update(path, expected_id, flow)
    }

    /// Rename a flow, such as from its tab, unless its file now holds
    /// another flow.
    pub(crate) fn rename(
        &mut self,
        path: &Path,
        expected_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), FlowLibraryError> {
        self.library.rename(path, expected_id, name)?;

        if let Some(saved) = self.library.get(path) {
            cx.emit(FlowPanelEvent::Renamed {
                path: path.to_path_buf(),
                name: saved.name.clone().into(),
            });
        }
        cx.notify();

        Ok(())
    }

    /// Select a flow and scroll its row into view.
    fn select(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.selected.as_ref() != Some(&path) {
            self.pending_delete = None;
        }

        if let Some(row) = self
            .library
            .flows()
            .iter()
            .position(|saved| saved.path == path)
        {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
        self.selected = Some(path);
        cx.notify();
    }

    fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(saved) = self.library.get(&path) {
            cx.emit(FlowPanelEvent::Open(saved.clone()));
        }
        self.select(path, cx);
    }

    /// Edit a flow's name in its row. Enter saves it; Escape or leaving the
    /// field keeps the name.
    pub(crate) fn begin_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(saved) = self.library.get(&path) else {
            return;
        };
        let name = saved.name.clone();
        self.error = None;
        self.select(path.clone(), cx);
        self.pending_delete = None;

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
        self.rename = Some(FlowRename {
            path,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        let name = rename.input.read(cx).value().trim().to_owned();
        let id = self.library.get(&rename.path).map(|saved| saved.id.clone());

        if let Some(id) = id
            && !name.is_empty()
            && let Err(error) = self.rename(&rename.path, &id, &name, cx)
        {
            self.error = Some(format!("Could not rename flow: {error}"));
        }

        // The new name can sort the flow elsewhere in the list.
        self.select(rename.path, cx);
        window.focus(&self.focus, cx);
    }

    fn duplicate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match self.library.duplicate(&path) {
            Ok(copy) => {
                self.error = None;
                self.open(copy, cx);
            }
            Err(error) => {
                self.error = Some(format!("Could not duplicate flow: {error}"));
                cx.notify();
            }
        }
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.pending_delete.take() {
            match self.library.delete(&path) {
                Ok(()) => {
                    self.error = None;
                    if self.selected.as_ref() == Some(&path) {
                        self.selected = None;
                    }
                    cx.emit(FlowPanelEvent::Deleted { path });
                }
                Err(error) => self.error = Some(format!("Could not delete flow: {error}")),
            }
        }

        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let flows = self.library.flows();
        if flows.is_empty() || event.keystroke.modifiers != Modifiers::default() {
            return;
        }

        let row = self
            .selected
            .as_ref()
            .and_then(|path| flows.iter().position(|saved| saved.path == *path));
        let path_at = |row: usize| flows[row].path.clone();
        let last = flows.len() - 1;

        match event.keystroke.key.as_str() {
            "down" => self.select(path_at(row.map_or(0, |row| (row + 1).min(last))), cx),
            "up" => self.select(path_at(row.unwrap_or(0).saturating_sub(1)), cx),
            "home" => self.select(path_at(0), cx),
            "end" => self.select(path_at(last), cx),
            "enter" if self.pending_delete.is_some() => self.confirm_delete(window, cx),
            "enter" => self.open(path_at(row.unwrap_or(0)), cx),
            "escape" if self.pending_delete.is_some() => {
                self.pending_delete = None;
                cx.notify();
            }
            _ => return,
        }

        cx.stop_propagation();
    }

    /// What right-clicking a flow or its "…" button offers.
    fn row_menu(&self, path: PathBuf, cx: &mut Context<Self>) -> MenuBuilder {
        let view = cx.entity().downgrade();

        Rc::new(move |menu, _, _| {
            let open_view = view.clone();
            let rename_view = view.clone();
            let duplicate_view = view.clone();
            let delete_view = view.clone();
            let open_path = path.clone();
            let rename_path = path.clone();
            let duplicate_path = path.clone();
            let delete_path = path.clone();
            let copied_path = path.to_string_lossy().into_owned();

            menu.item(PopupMenuItem::new("Open").on_click(move |_, _, cx| {
                let _ = open_view.update(cx, |this, cx| this.open(open_path.clone(), cx));
            }))
            .item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                let view = rename_view.clone();
                let path = rename_path.clone();
                window.defer(cx, move |window, cx| {
                    let _ = view.update(cx, |this, cx| this.begin_rename(path, window, cx));
                });
            }))
            .item(PopupMenuItem::new("Duplicate").on_click(move |_, _, cx| {
                let _ = duplicate_view
                    .update(cx, |this, cx| this.duplicate(duplicate_path.clone(), cx));
            }))
            .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied_path.clone()));
            }))
            .separator()
            .item(
                PopupMenuItem::new("Delete Flow").on_click(move |_, window, cx| {
                    let view = delete_view.clone();
                    let path = delete_path.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.pending_delete = Some(path);
                            window.focus(&this.focus, cx);
                            cx.notify();
                        });
                    });
                }),
            )
        })
    }

    fn delete_prompt(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(("flow-row", index))
            .debug_selector(move || format!("flow-row-{index}"))
            .h_8()
            .w_full()
            .px_2()
            .child(
                h_flex()
                    .debug_selector(|| "flow-delete-prompt".into())
                    .size_full()
                    .rounded(cx.theme().radius_tokens().md)
                    .px_2()
                    .gap_1()
                    .bg(cx.theme().sidebar_accent)
                    .child(div().flex_1().min_w_0().text_xs().child("Delete flow?"))
                    .child(
                        Button::new("confirm-flow-delete")
                            .debug_selector(|| "confirm-flow-delete".into())
                            .label("Delete")
                            .tooltip("Confirm deletion (Enter)")
                            .xsmall()
                            .danger()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_delete(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-flow-delete")
                            .debug_selector(|| "cancel-flow-delete".into())
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

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let saved = &self.library.flows()[index];
        let path = saved.path.clone();
        let name: SharedString = saved.name.clone().into();

        if self.pending_delete.as_ref() == Some(&path) {
            return self.delete_prompt(index, cx);
        }

        let menu = self.row_menu(path.clone(), cx);
        let theme = cx.theme();
        let selected = self.selected.as_ref() == Some(&path);
        let menu_open = self.menu_row.as_ref() == Some(&path);
        let rename = self.rename.as_ref().filter(|rename| rename.path == path);
        let menu_view = cx.entity().downgrade();
        let menu_path = path.clone();
        let click_path = path.clone();
        let select_path = path.clone();

        div()
            .id(("flow-row", index))
            .debug_selector(move || format!("flow-row-{index}"))
            .group("flow-row")
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
                        Icon::default()
                            .path("icons/workflow.svg")
                            .size(rems(0.875))
                            .flex_none()
                            .text_color(theme.chart_4),
                    )
                    .child(match rename {
                        Some(rename) => div()
                            .id("flow-rename-editor")
                            .debug_selector(|| "flow-rename-editor".into())
                            .flex_1()
                            .min_w_0()
                            .capture_key_down(cx.listener(
                                |this, event: &KeyDownEvent, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        this.rename = None;
                                        window.focus(&this.focus, cx);
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(Input::new(&rename.input).small())
                            .into_any_element(),
                        None => div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(name.clone())
                            .into_any_element(),
                    })
                    .when(rename.is_none(), |this| {
                        // Shown while the row is hovered, selected or its
                        // menu is open, as the context menu's twin.
                        this.child(
                            div()
                                .id(("flow-row-menu-slot", index))
                                .flex_none()
                                .when(!selected && !menu_open, |this| {
                                    this.invisible()
                                        .group_hover("flow-row", |this| this.visible())
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    Button::new(("flow-row-menu", index))
                                        .debug_selector(move || format!("flow-row-menu-{index}"))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Ellipsis)
                                        .accessibility_label(format!("More actions for {name}"))
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
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.open(click_path.clone(), cx);
            }))
            .capture_any_mouse_down(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.button == MouseButton::Right {
                        window.focus(&this.focus, cx);
                        this.select(select_path.clone(), cx);
                    }
                }),
            )
            .context_menu(move |popup, window, cx| menu(popup, window, cx))
            .into_any_element()
    }
}

impl Focusable for FlowPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for FlowPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.library.flows().len();
        let skipped = self.library.skipped().len();

        v_flex()
            .debug_selector(|| "flows-sidebar".into())
            .size_full()
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            // Only the count: request-eagle-cli tells agents which files and why.
            .when(skipped > 0, |this| {
                this.child(
                    h_flex()
                        .debug_selector(|| "flows-skipped".into())
                        .px_3()
                        .pb_2()
                        .gap_1()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(Icon::new(IconName::TriangleAlert).xsmall())
                        .child(match skipped {
                            1 => "Couldn't load 1 file".to_owned(),
                            count => format!("Couldn't load {count} files"),
                        }),
                )
            })
            .child(
                div()
                    .id("flows-list")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(if count == 0 {
                        v_flex()
                            .p_4()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No flows yet")
                            .child(
                                "Create one to chain requests on a canvas, without writing a \
                                 script.",
                            )
                            .into_any_element()
                    } else {
                        // Only the rows in view are built, however many flows there are.
                        uniform_list(
                            "flows-scroll",
                            count,
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.row(index, cx)).collect()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.scroll)
                        .into_any_element()
                    })
                    .when(count > 0, |this| {
                        this.child(Scrollbar::vertical(&self.scroll))
                    }),
            )
    }
}
