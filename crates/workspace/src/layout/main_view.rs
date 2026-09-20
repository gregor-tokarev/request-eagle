use std::path::{Path, PathBuf};

use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{button::*, *};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::request_draft::{MethodChanged, RequestDraft};
use crate::actions::{CloseTab, NewTab};

const TAB_WIDTH: Pixels = px(176.);
const TAB_HEIGHT: Pixels = px(28.);

pub(super) struct PageTab {
    pub(super) id: u64,
    pub(super) title: SharedString,
    pub(super) request_path: Option<PathBuf>,
    pub(super) method: Option<&'static str>,
    pub(super) page: AnyView,
    _request_subscription: Option<Subscription>,
}

pub(crate) struct MainView {
    pub(super) tabs: Vec<PageTab>,
    pub(super) selected: Option<usize>,
    next_id: u64,
    scroll: ScrollHandle,
    scroll_to_tab: Option<usize>,
    focus: FocusHandle,
}

struct EmptyPage;

impl Render for EmptyPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

impl MainView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            tabs: Vec::new(),
            selected: None,
            next_id: 1,
            scroll: ScrollHandle::new(),
            scroll_to_tab: None,
            focus: cx.focus_handle(),
        };

        view.new_tab(cx);

        view
    }

    /// Each tab owns its page entity, preserving page state when switching tabs.
    pub(crate) fn open_tab(
        &mut self,
        title: impl Into<SharedString>,
        page: impl Into<AnyView>,
        cx: &mut Context<Self>,
    ) -> usize {
        self.tabs.push(PageTab {
            id: self.next_id,
            title: title.into(),
            request_path: None,
            method: None,
            page: page.into(),
            _request_subscription: None,
        });
        self.next_id += 1;

        let index = self.tabs.len() - 1;
        self.select_tab(index, cx);

        index
    }

    pub(crate) fn open_request(
        &mut self,
        path: &Path,
        name: SharedString,
        method: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.request_path.as_deref() == Some(path))
        {
            self.tabs[index].title = name;
            self.tabs[index].method = Some(method);
            self.select_tab(index, cx);

            return;
        }

        let page = cx.new(|_| EmptyPage);
        let index = self.open_tab(name, page, cx);
        self.tabs[index].request_path = Some(path.to_path_buf());
        self.tabs[index].method = Some(method);
    }

    pub(crate) fn new_tab(&mut self, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.next_id);
        let page = cx.new(|_| RequestDraft::new());
        let id = self.next_id;
        let subscription = cx.subscribe(&page, move |this, _, event: &MethodChanged, cx| {
            if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                tab.method = Some(event.0.as_str());
                cx.notify();
            }
        });

        let index = self.open_tab(title, page, cx);
        self.tabs[index].method = Some("GET");
        self.tabs[index]._request_subscription = Some(subscription);
    }

    /// Select by zero-based position. Missing positions leave selection unchanged.
    pub(crate) fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        self.selected = Some(index);
        self.scroll_to_tab = Some(index);

        cx.notify();
    }

    pub(crate) fn cycle_tab(&mut self, previous: bool, cx: &mut Context<Self>) {
        let Some(index) = self.selected else {
            return;
        };

        let count = self.tabs.len();
        let next = if previous {
            (index + count - 1) % count
        } else {
            (index + 1) % count
        };

        self.select_tab(next, cx);
    }

    pub(crate) fn select_last_tab(&mut self, cx: &mut Context<Self>) {
        self.select_tab(self.tabs.len().saturating_sub(1), cx);
    }

    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.close_tab(index, cx);
        }
    }

    pub(super) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        self.tabs.remove(index);
        self.selected = self.selected.and_then(|selected| {
            if self.tabs.is_empty() {
                None
            } else if index < selected {
                Some(selected - 1)
            } else {
                Some(selected.min(self.tabs.len() - 1))
            }
        });

        self.scroll_to_tab = self.selected;

        cx.notify();
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected
            && let Ok(draft) = self.tabs[index].page.clone().downcast::<RequestDraft>()
        {
            draft.update(cx, |draft, cx| draft.prepare(window, cx));
        }

        window.focus(&self.focus, cx);
    }

    fn tab_strip(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let gap = window.rem_size() * 0.25;
        let stride = TAB_WIDTH + gap;
        let content_width = TAB_WIDTH * self.tabs.len() + gap * self.tabs.len().saturating_sub(1);

        Tabs::new("page-tabs")
            .min_w_0()
            .flex_shrink(1.)
            .flex()
            .overflow_x_scroll()
            .track_scroll(&self.scroll)
            .child(
                // Reserve the full scroll extent, but build and lay out only the
                // visible tabs. GPUI's uniform_list only supports vertical lists.
                canvas(
                    cx.processor(move |this, bounds: Bounds<Pixels>, window, cx| {
                        // The parent has its current viewport and clamped scroll
                        // offset now, including on the first frame and after resize.
                        let viewport = this.scroll.bounds();
                        let mut left = -this.scroll.offset().x;

                        if let Some(index) = this.scroll_to_tab.take() {
                            let tab_left = stride * index;
                            let tab_right = tab_left + TAB_WIDTH;

                            if tab_left < left || TAB_WIDTH > viewport.size.width {
                                left = tab_left;
                            } else if tab_right > left + viewport.size.width {
                                left = tab_right - viewport.size.width;
                            }
                        }

                        left =
                            left.clamp(px(0.), (content_width - viewport.size.width).max(px(0.)));
                        this.scroll.set_offset(point(-left, px(0.)));

                        let first = (left / stride).floor() as usize;
                        let end = (((left + viewport.size.width) / stride).ceil() as usize)
                            .min(this.tabs.len());
                        let mut tabs = Vec::with_capacity(end.saturating_sub(first));

                        for index in first..end {
                            let mut tab = this.tab(index, &this.tabs[index], cx).into_any_element();
                            tab.layout_as_root(
                                size(
                                    AvailableSpace::Definite(TAB_WIDTH),
                                    AvailableSpace::Definite(TAB_HEIGHT),
                                ),
                                window,
                                cx,
                            );
                            // Use the new offset immediately, so keyboard jumps
                            // reveal the selected tab in this frame.
                            tab.prepaint_at(
                                point(viewport.left() - left + stride * index, bounds.top()),
                                window,
                                cx,
                            );
                            tabs.push(tab);
                        }

                        tabs
                    }),
                    |_, tabs, window, cx| {
                        for mut tab in tabs {
                            tab.paint(window, cx);
                        }
                    },
                )
                .flex_none()
                .w(content_width)
                .h(TAB_HEIGHT),
            )
    }

    fn tab(&self, index: usize, tab: &PageTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.selected == Some(index);
        let id = tab.id;

        Tab::new(("page-tab", id))
            .debug_selector(move || format!("page-tab-{id}"))
            .group("page-tab")
            .selected(selected)
            .accessibility_label(tab.title.clone())
            .set_position(index + 1, self.tabs.len())
            .flex_none()
            .w(TAB_WIDTH)
            .h(TAB_HEIGHT)
            .px_2()
            .gap_2()
            .rounded(px(5.))
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .when(selected, |this| {
                this.bg(cx.theme().tab_active)
                    .text_color(cx.theme().foreground)
            })
            .hover(|this| {
                if selected {
                    this
                } else {
                    this.bg(cx.theme().muted)
                }
            })
            .when_some(tab.method, |this, method| {
                let color = match method {
                    "GET" => cx.theme().success,
                    "POST" => cx.theme().warning,
                    "PUT" => cx.theme().info,
                    _ => cx.theme().danger,
                };

                this.child(
                    div()
                        .debug_selector(move || format!("tab-method-{id}"))
                        .flex_none()
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(method),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(tab.title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .invisible()
                    .group_hover("page-tab", |this| this.visible())
                    .child(
                        Button::new(("close-tab", id))
                            .debug_selector(move || format!("close-tab-{id}"))
                            .ghost()
                            .xsmall()
                            .size(px(20.))
                            .icon(Icon::new(IconName::Close).size(px(12.)))
                            .accessibility_label(format!("Close {}", tab.title))
                            .tooltip_with_action("Close tab", &CloseTab, Some("Workspace"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(index, cx);
                                this.focus(window, cx);
                            })),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_tab(index, cx);
                this.focus(window, cx);
            }))
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "main-view".into())
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .debug_selector(|| "main-tab-bar".into())
                    .flex_none()
                    .h(px(38.))
                    .px_1()
                    .gap_1()
                    .bg(cx.theme().tab_bar)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(self.tab_strip(window, cx))
                    .child(
                        Button::new("new-tab")
                            .debug_selector(|| "new-tab".into())
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(IconName::Plus)
                            .accessibility_label("New tab")
                            .tooltip_with_action("New tab", &NewTab, Some("Workspace"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_tab(cx);
                                this.focus(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("tab-content")
                    .role(Role::TabPanel)
                    .debug_selector(|| "tab-content".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // Focus the page before its controls handle the click, so
                    // clicking an input can keep focus instead of losing it here.
                    .capture_any_mouse_down(cx.listener(
                        |this, event: &MouseDownEvent, window, cx| {
                            if event.button == MouseButton::Left {
                                this.focus(window, cx);
                            }
                        },
                    ))
                    .when_some(self.selected, |this, index| {
                        this.aria_label(self.tabs[index].title.clone())
                            // Scrolling/hovering tabs must not lay out an unchanged
                            // editor. Child notifications and resize invalidate it.
                            .child(
                                self.tabs[index]
                                    .page
                                    .clone()
                                    .cached(StyleRefinement::default().size_full()),
                            )
                    }),
            )
    }
}
