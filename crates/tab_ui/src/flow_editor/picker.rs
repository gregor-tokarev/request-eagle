use flow::{BlockKind, BlockType};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName,
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use super::{
    FlowEditor,
    blocks::{color, icon},
    editor::{FlowRequest, PortRef},
    geometry,
};

/// The most saved requests the picker lists.
const REQUEST_LIMIT: usize = 8;

/// How far down the picker its search field's middle is: its padding and
/// half the field.
const SEARCH_MIDDLE: Rems = rems(1.25);

/// Chooses a block to add, or a saved request to send from a new HTTP
/// Request block. Opened at the canvas position the block goes to.
pub(super) struct Picker {
    pub position: Point<f32>,
    /// The port a connection was drawn from, which the new block joins.
    pub from: Option<PortRef>,
    pub search: Entity<InputState>,
    pub highlighted: usize,
    /// Keeps the highlighted item in view.
    scroll: ScrollHandle,
    _subscription: Subscription,
}

#[derive(Clone)]
pub(super) enum PickerItem {
    Block(BlockType),
    Request(Box<FlowRequest>),
}

impl Picker {
    pub fn new(
        position: Point<f32>,
        from: Option<PortRef>,
        window: &mut Window,
        cx: &mut Context<FlowEditor>,
    ) -> Self {
        let search = cx.new(|cx| {
            let input = InputState::new(window, cx).placeholder("Search blocks or requests");
            input.focus(window, cx);
            input
        });
        let subscription = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(picker) = &mut this.picker
            {
                picker.highlighted = 0;
                cx.notify();
            }
        });

        Self {
            position,
            from,
            search,
            highlighted: 0,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }
}

impl FlowEditor {
    /// The blocks and requests that match the picker's search, blocks first.
    fn picker_items(&self, picker: &Picker, cx: &App) -> Vec<PickerItem> {
        let query = picker.search.read(cx).value().trim().to_lowercase();
        let matches = |text: &str| text.to_lowercase().contains(&query);

        let mut items: Vec<PickerItem> = BlockType::ALL
            .iter()
            .copied()
            // Start blocks begin a flow; a connection cannot lead into one.
            .filter(|block_type| {
                picker.from.as_ref().is_none_or(|from| {
                    let kind = block_type.block_kind();
                    let ports = if from.output {
                        kind.inputs()
                    } else {
                        kind.outputs()
                    };
                    !ports.is_empty()
                })
            })
            .filter(|block_type| {
                query.is_empty() || matches(block_type.name()) || matches(block_type.description())
            })
            .map(PickerItem::Block)
            .collect();
        // Blocks named like the search come before those that only describe it.
        items.sort_by_key(|item| match item {
            PickerItem::Block(block_type) => !matches(block_type.name()),
            PickerItem::Request(_) => true,
        });

        if picker.from.as_ref().is_none_or(|from| from.output) {
            items.extend(
                self.requests
                    .all(cx)
                    .into_iter()
                    .filter(|request| {
                        query.is_empty()
                            || matches(&request.name)
                            || matches(&request.request.path)
                            || matches(&request.location)
                    })
                    .take(REQUEST_LIMIT)
                    .map(|request| PickerItem::Request(Box::new(request))),
            );
        }

        items
    }

    fn pick(&mut self, item: PickerItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.take() else {
            return;
        };

        let kind = match item {
            PickerItem::Block(block_type) => block_type.block_kind(),
            PickerItem::Request(request) => BlockKind::HttpRequest {
                request: request.id,
            },
        };
        // A connection drawn to the picker reaches the new block's first
        // port where the connection was dropped.
        let position = match &picker.from {
            Some(from) if from.output => point(
                picker.position.x,
                picker.position.y - geometry::port_offset(0),
            ),
            Some(_) => point(
                picker.position.x - geometry::width(&kind),
                picker.position.y - geometry::port_offset(0),
            ),
            None => picker.position,
        };
        self.add_block(kind, position, picker.from, window, cx);
        window.focus(&self.focus, cx);
    }

    fn on_picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = &self.picker else {
            return;
        };
        let count = self.picker_items(picker, cx).len();

        match event.keystroke.key.as_str() {
            "escape" => {
                self.picker = None;
                window.focus(&self.focus, cx);
            }
            "down" if count > 0 => {
                if let Some(picker) = &mut self.picker {
                    picker.highlighted = (picker.highlighted + 1) % count;
                    picker.scroll.scroll_to_item(picker.highlighted);
                }
            }
            "up" if count > 0 => {
                if let Some(picker) = &mut self.picker {
                    picker.highlighted = (picker.highlighted + count - 1) % count;
                    picker.scroll.scroll_to_item(picker.highlighted);
                }
            }
            "enter" => {
                let item = self.picker.as_ref().and_then(|picker| {
                    self.picker_items(picker, cx)
                        .into_iter()
                        .nth(picker.highlighted)
                });
                if let Some(item) = item {
                    self.pick(item, window, cx);
                }
            }
            _ => return,
        }

        cx.stop_propagation();
        cx.notify();
    }

    /// Where the picker is drawn, relative to the canvas. It opens beside
    /// the spot it adds a block at: to the right, or to the left when a
    /// connection was drawn from an input. It stays inside the canvas,
    /// however small that is.
    fn picker_bounds(&self, picker: &Picker) -> Bounds<Pixels> {
        let margin = px(8.);
        let width = rems(18.)
            .to_pixels(self.rem)
            .min(self.view.size.width - margin * 2.);
        let height = rems(24.)
            .to_pixels(self.rem)
            .min(self.view.size.height - margin * 2.);
        let at = self.viewport.to_view(picker.position, self.rem);
        let from_input = picker.from.as_ref().is_some_and(|from| !from.output);
        let left = if from_input { at.x - width } else { at.x };
        let left = left.min(self.view.size.width - width - margin).max(margin);
        let top = (at.y - SEARCH_MIDDLE.to_pixels(self.rem))
            .min(self.view.size.height - height - margin)
            .max(margin);

        Bounds::new(point(left, top), size(width, height))
    }

    /// Where a connection drawn to the picker ends: the middle of its
    /// search field's edge that faces the connection's port.
    pub(super) fn picker_anchor(&self, picker: &Picker) -> Option<Point<f32>> {
        let from = picker.from.as_ref()?;
        let bounds = self.picker_bounds(picker);
        let x = if from.output {
            bounds.left()
        } else {
            bounds.right()
        };
        let y = bounds.top() + SEARCH_MIDDLE.to_pixels(self.rem);

        Some(self.viewport.to_canvas(point(x, y), self.rem))
    }

    pub(super) fn render_picker(
        &self,
        picker: &Picker,
        _: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let items = self.picker_items(picker, cx);
        let bounds = self.picker_bounds(picker);
        let first_request = items
            .iter()
            .position(|item| matches!(item, PickerItem::Request(_)));

        v_flex()
            .id("flow-block-picker")
            .debug_selector(|| "flow-block-picker".into())
            .absolute()
            .left(bounds.left())
            .top(bounds.top())
            .w(bounds.size.width)
            .max_h(bounds.size.height)
            .p_1()
            .gap_1()
            .rounded(theme.radius_tokens().lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(Self::on_picker_key_down))
            .child(
                Input::new(&picker.search)
                    .prefix(IconName::Search)
                    .aria_label("Search blocks or requests"),
            )
            .child(
                v_flex()
                    .id("flow-block-picker-items")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .when(items.is_empty(), |this| {
                        this.child(
                            div()
                                .p_2()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("No blocks or requests match"),
                        )
                    })
                    .children(items.into_iter().enumerate().map(|(index, item)| {
                        let highlighted = index == picker.highlighted;
                        let heading = (Some(index) == first_request).then(|| {
                            div()
                                .px_2()
                                .pt_2()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Saved requests")
                        });
                        let row =
                            h_flex()
                                .id(("flow-picker-item", index))
                                .debug_selector(move || format!("flow-picker-item-{index}"))
                                .h_8()
                                .px_2()
                                .gap_2()
                                .rounded(theme.radius_tokens().md)
                                .text_sm()
                                .when(highlighted, |this| this.bg(theme.accent))
                                .hover(|this| this.bg(theme.accent.opacity(0.7)))
                                .on_click(cx.listener({
                                    let item = item.clone();
                                    move |this, _, window, cx| this.pick(item.clone(), window, cx)
                                }))
                                .child(match &item {
                                    PickerItem::Block(block_type) => h_flex()
                                        .gap_2()
                                        .min_w_0()
                                        .child(
                                            Icon::default()
                                                .path(icon(*block_type))
                                                .size_4()
                                                .flex_none()
                                                .text_color(color(*block_type, cx)),
                                        )
                                        .child(div().flex_none().child(block_type.name()))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .text_xs()
                                                .text_ellipsis()
                                                .text_color(theme.muted_foreground)
                                                .child(block_type.description()),
                                        ),
                                    PickerItem::Request(request) => h_flex()
                                        .gap_2()
                                        .min_w_0()
                                        .child(div().flex_none().child(method_label(
                                            request.request.method.as_str(),
                                            cx,
                                        )))
                                        .child(div().flex_none().child(request.name.clone()))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .text_xs()
                                                .text_ellipsis()
                                                .text_color(theme.muted_foreground)
                                                .child(request.location.clone()),
                                        ),
                                });

                        div().children(heading).child(row)
                    })),
            )
            .into_any_element()
    }
}
