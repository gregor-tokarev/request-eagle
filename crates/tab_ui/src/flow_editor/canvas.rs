use std::time::Duration;

use flow::BlockKind;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    FlowEditor,
    actions::*,
    editor::{Drag, Hover, PortRef, connection_between},
    geometry,
    picker::Picker,
    zoom::Zoom,
};
use crate::SendRequest;

/// Canvas pixels between the dots of the background grid.
const GRID: f32 = 24.;
/// How close to the canvas's edge a drag pans it, in screen pixels, and how
/// fast at most, in screen pixels a frame.
const EDGE: f32 = 40.;
const EDGE_SPEED: f32 = 18.;
/// The smallest a Note can be resized to.
const NOTE_MIN: Size<f32> = Size {
    width: 160.,
    height: 72.,
};

/// The curves of a connection, with the color and width to draw them.
type WirePaint = (Vec<[Point<f32>; 4]>, Hsla, f32);

/// A connection being drawn, from its port to where it ends.
struct PendingWire {
    curve: [Point<f32>; 4],
    end: Point<f32>,
    kind: WireEnd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WireEnd {
    /// Following the pointer over empty canvas.
    Pointer,
    /// Over a port, or a block, that it would join.
    Port,
    /// At the block picker it opened, which adds the block it joins.
    Picker,
}

impl FlowEditor {
    /// The canvas position under a point of the window.
    pub(super) fn canvas_position(&self, position: Point<Pixels>) -> Point<f32> {
        self.viewport
            .to_canvas(position - self.view.origin, self.rem)
    }

    /// A press on a block, a port or the canvas starts what dragging does
    /// there: drawing a connection, moving blocks, selecting or panning.
    pub(super) fn press(
        &mut self,
        event: &MouseDownEvent,
        block: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.picker = None;
        let position = self.canvas_position(event.position);
        let additive = event.modifiers.shift || event.modifiers.secondary();

        if let Some(port) = self
            .port_at(position, true)
            .or_else(|| self.port_at(position, false))
        {
            // Pulling a connected input's connection off moves its end.
            let detached = (!port.output)
                .then(|| self.flow.connection_into(&port.block, &port.port).cloned())
                .flatten();
            let from = match &detached {
                Some(connection) => PortRef {
                    block: connection.from.clone(),
                    port: connection.output.clone(),
                    output: true,
                },
                None => port,
            };
            self.drag = Some(Drag::Connect {
                from,
                pointer: position,
                detached,
            });
            self.start_edge_pan(window, cx);
            cx.notify();
            return;
        }

        let block = block.or_else(|| self.block_at(position));
        if let Some(id) = block {
            if event.click_count >= 2 {
                self.set_selection(vec![id.clone()], window, cx);
                self.focus_inspector(window, cx);
                return;
            }

            self.select(&id, additive, window, cx);
            if self.selection.contains(&id) {
                let origins = self
                    .moving_blocks()
                    .into_iter()
                    .filter_map(|id| {
                        let block = self.flow.block(&id)?;
                        let origin = point(block.x, block.y);
                        Some((id, origin))
                    })
                    .collect();
                self.drag = Some(Drag::Move {
                    start: position,
                    origins,
                    moved: false,
                });
                self.start_edge_pan(window, cx);
            }
            return;
        }

        if let Some(connection) = self.connection_at(position) {
            self.selection.clear();
            self.selected_connection = Some(connection);
            self.sync_inspector(window, cx);
            cx.notify();
            return;
        }

        if event.modifiers.shift {
            self.drag = Some(Drag::Select {
                start: position,
                end: position,
                kept: self.selection.clone(),
            });
            self.start_edge_pan(window, cx);
        } else {
            if !self.selection.is_empty() || self.selected_connection.is_some() {
                self.set_selection(Vec::new(), window, cx);
            }
            self.drag = Some(Drag::Pan {
                last: event.position,
            });
        }
        cx.notify();
    }

    /// The selected blocks, and the blocks inside the selected Notes, which
    /// move with them.
    fn moving_blocks(&self) -> Vec<String> {
        let mut moving = self.selection.clone();
        for id in &self.selection {
            let Some(frame) = self
                .flow
                .block(id)
                .filter(|block| matches!(block.kind, BlockKind::Note { .. }))
                .and_then(|block| self.layouts.get(&block.id))
            else {
                continue;
            };
            for block in &self.flow.blocks {
                if !moving.contains(&block.id)
                    && self
                        .layouts
                        .get(&block.id)
                        .is_some_and(|layout| geometry::contains(&frame.bounds, &layout.bounds))
                {
                    moving.push(block.id.clone());
                }
            }
        }
        moving
    }

    /// Begin resizing a Note from its corner.
    pub(super) fn start_resize(
        &mut self,
        id: &str,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.picker = None;
        let Some(layout) = self.layouts.get(id) else {
            return;
        };
        let size = layout.bounds.size;

        self.select(id, false, window, cx);
        self.drag = Some(Drag::Resize {
            block: id.to_owned(),
            start: self.canvas_position(event.position),
            size,
            resized: false,
        });
        self.start_edge_pan(window, cx);
        cx.notify();
    }

    /// Pan the canvas while a drag is held near its edge, so blocks and
    /// connections can be taken past what is in view.
    fn start_edge_pan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edge_pan = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let going = this
                    .update_in(cx, |this, window, cx| this.edge_pan_step(window, cx))
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        }));
    }

    /// Pan a step toward the edge the pointer is near. Returns whether the
    /// drag goes on.
    fn edge_pan_step(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.drag.is_none() {
            return false;
        }
        let Some(pointer) = self.pointer else {
            return true;
        };

        let speed = |inside: Pixels| {
            let inside = f32::from(inside);
            if inside < EDGE {
                EDGE_SPEED * (1. - inside.max(0.) / EDGE)
            } else {
                0.
            }
        };
        let bounds = self.view;
        let dx = speed(pointer.x - bounds.left()) - speed(bounds.right() - pointer.x);
        let dy = speed(pointer.y - bounds.top()) - speed(bounds.bottom() - pointer.y);
        if dx == 0. && dy == 0. {
            return true;
        }

        self.viewport.pan(point(px(dx), px(dy)), self.rem);
        self.drag_to(pointer, window, cx);
        cx.notify();
        true
    }

    /// Note what the pointer is over, to highlight its connections.
    fn hover_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.drag.is_some() {
            return;
        }

        let at = self.canvas_position(position);
        let hover = match self.block_at(at) {
            Some(block) => Some(Hover::Block(block)),
            None => self.connection_at(at).map(Hover::Connection),
        };
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    fn drag_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        let pointer = self.canvas_position(position);

        match &mut self.drag {
            Some(Drag::Pan { last }) => {
                let delta = position - *last;
                *last = position;
                self.viewport.pan(delta, self.rem);
            }
            Some(Drag::Move {
                start,
                origins,
                moved,
            }) => {
                let delta = point(pointer.x - start.x, pointer.y - start.y);
                if !*moved {
                    // Ignore a click that barely moves.
                    let threshold = 3. / self.viewport.scale(self.rem);
                    if delta.x.abs() < threshold && delta.y.abs() < threshold {
                        return;
                    }
                    *moved = true;
                    self.history.record(&self.flow, None);
                }

                for (id, origin) in origins.iter() {
                    if let Some(block) = self.flow.block_mut(id) {
                        block.x = (origin.x + delta.x).round();
                        block.y = (origin.y + delta.y).round();
                    }
                }
            }
            Some(Drag::Connect { pointer: end, .. }) => *end = pointer,
            Some(Drag::Resize {
                block,
                start,
                size,
                resized,
            }) => {
                if !*resized {
                    *resized = true;
                    self.history.record(&self.flow, None);
                }
                let width = (size.width + pointer.x - start.x)
                    .max(NOTE_MIN.width)
                    .round();
                let height = (size.height + pointer.y - start.y)
                    .max(NOTE_MIN.height)
                    .round();
                let id = block.clone();
                if let Some(BlockKind::Note {
                    width: note_width,
                    height: note_height,
                    ..
                }) = self.flow.block_mut(&id).map(|block| &mut block.kind)
                {
                    *note_width = Some(width);
                    *note_height = Some(height);
                }
            }
            Some(Drag::Select { start, end, kept }) => {
                *end = pointer;
                let area = geometry::rectangle(*start, *end);
                let mut selection = kept.clone();
                for block in &self.flow.blocks {
                    // A Note is selected only whole, so a box drawn inside
                    // the section it frames picks the blocks there.
                    let reached = |bounds: &Bounds<f32>| match block.kind {
                        BlockKind::Note { .. } => geometry::contains(&area, bounds),
                        _ => geometry::intersects(&area, bounds),
                    };
                    if let Some(layout) = self.layouts.get(&block.id)
                        && reached(&layout.bounds)
                        && !selection.contains(&block.id)
                    {
                        selection.push(block.id.clone());
                    }
                }
                self.selection = selection;
                self.sync_inspector(window, cx);
            }
            None => return,
        }

        cx.notify();
    }

    fn release(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        self.edge_pan = None;
        let pointer = self.canvas_position(position);

        match drag {
            Drag::Connect { from, detached, .. } => {
                let on_canvas = self.block_at(pointer).is_none()
                    && self.port_at(pointer, !from.output).is_none();

                match (self.drop_port(&from, pointer), detached) {
                    (Some(to), Some(connection)) => {
                        if let Some(moved) = connection_between(from, to)
                            && moved != connection
                        {
                            self.edit(
                                None,
                                |flow| {
                                    flow.connections.retain(|existing| *existing != connection);
                                    flow.connect(moved);
                                },
                                cx,
                            );
                        }
                    }
                    (Some(to), None) => self.connect(from, to, cx),
                    // A connection pulled off and dropped on empty canvas is removed.
                    (None, Some(connection)) if on_canvas => self.edit(
                        None,
                        |flow| flow.connections.retain(|existing| *existing != connection),
                        cx,
                    ),
                    // Dropped on empty canvas: choose a block to connect to.
                    (None, None) if on_canvas => self.open_picker(pointer, Some(from), window, cx),
                    // Dropped on a block or port it cannot join, such as its
                    // own: nothing changes.
                    (None, _) => {}
                }
            }
            Drag::Move { moved: true, .. } => {
                self.history.seal();
                // Blocks moved by hand stay where they were put.
                self.pending_reveal = None;
            }
            Drag::Resize { resized: true, .. } => {
                self.history.seal();
                self.pending_reveal = None;
            }
            _ => {}
        }

        cx.notify();
    }

    /// The port a connection drawn from `from` joins when dropped at a
    /// position: a port near it, or the first free port of the block there.
    /// A block cannot connect to itself.
    pub(super) fn drop_port(&self, from: &PortRef, position: Point<f32>) -> Option<PortRef> {
        self.port_at(position, !from.output)
            .or_else(|| {
                self.block_at(position)
                    .and_then(|block| self.free_port(&block, !from.output))
            })
            .filter(|port| port.block != from.block)
    }

    /// Where a port is on the canvas.
    fn port_position(&self, port: &PortRef) -> Option<Point<f32>> {
        let layout = self.layouts.get(&port.block)?;
        let ports = if port.output {
            &layout.outputs
        } else {
            &layout.inputs
        };
        let index = ports.iter().position(|name| *name == port.port)?;

        Some(if port.output {
            layout.output_position(index)
        } else {
            layout.input_position(index)
        })
    }

    /// The connection being drawn, from its port to where it ends: the
    /// pointer, the port it would join, or the block picker it opened.
    fn pending_wire(&self) -> Option<PendingWire> {
        let (from, end, kind) = match (&self.drag, &self.picker) {
            (Some(Drag::Connect { from, pointer, .. }), _) => {
                match self
                    .drop_port(from, *pointer)
                    .and_then(|port| self.port_position(&port))
                {
                    Some(port) => (from, port, WireEnd::Port),
                    None => (from, *pointer, WireEnd::Pointer),
                }
            }
            (None, Some(picker)) => {
                let from = picker.from.as_ref()?;
                (from, self.picker_anchor(picker)?, WireEnd::Picker)
            }
            _ => return None,
        };
        let start = self.port_position(from)?;
        let curve = if from.output {
            geometry::wire(start, end)
        } else {
            geometry::wire(end, start)
        };

        Some(PendingWire { curve, end, kind })
    }

    /// The first port of a block on the given side that has no connection,
    /// or its first port.
    fn free_port(&self, block: &str, output: bool) -> Option<PortRef> {
        let layout = self.layouts.get(block)?;
        let ports = if output {
            &layout.outputs
        } else {
            &layout.inputs
        };
        let port = ports
            .iter()
            .find(|port| output || self.flow.connection_into(block, port).is_none())
            .or(ports.first())?;

        Some(PortRef {
            block: block.to_owned(),
            port: port.to_string(),
            output,
        })
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let wheel = geometry::from_wheel(event.delta, cx.compositor_name() == "X11");
        self.viewport.scroll(
            event.delta,
            wheel,
            event.modifiers,
            event.position - self.view.origin,
            window.line_height(),
            self.rem,
        );

        cx.stop_propagation();
        cx.notify();
    }

    /// Pinching a trackpad zooms around the fingers.
    fn pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        let zoom = self.viewport.zoom * (1. + event.delta);
        self.viewport
            .zoom_around(zoom, event.position - self.view.origin, self.rem);

        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn open_picker(
        &mut self,
        position: Point<f32>,
        from: Option<PortRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = None;
        self.picker = Some(Picker::new(position, from, window, cx));
        cx.notify();
    }

    pub(super) fn render_canvas(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity().downgrade();
        let dragging = self.drag.is_some();
        let scale = self.viewport.scale(self.rem);

        // Only blocks in view are built; connections are drawn from layouts.
        // Notes are built first, so they lie behind the blocks they frame.
        let visible = (self.view.size.width > px(0.))
            .then(|| self.viewport.visible(self.view.size, self.rem));
        let (notes, others): (Vec<_>, Vec<_>) = self
            .flow
            .blocks
            .iter()
            .partition(|block| matches!(block.kind, BlockKind::Note { .. }));
        let blocks: Vec<AnyElement> = notes
            .into_iter()
            .chain(others)
            .filter_map(|block| {
                let layout = self.layouts.get(&block.id)?;
                if let Some(visible) = &visible
                    && !geometry::intersects(visible, &layout.bounds)
                {
                    return None;
                }
                Some(self.block_element(block, layout, cx))
            })
            .collect();

        let (wires, emphasized) = self.wire_paths(cx);
        let theme = cx.theme();
        let pending = self.pending_wire();
        let marquee = match &self.drag {
            Some(Drag::Select { start, end, .. }) => Some(geometry::rectangle(*start, *end)),
            _ => None,
        };
        let viewport = self.viewport;
        let rem = self.rem;
        let grid_color = theme.muted_foreground.opacity(0.22);
        let marquee_color = theme.ring;
        let pending_color = theme.ring;

        div()
            .id("flow-canvas")
            .debug_selector(|| "flow-canvas".into())
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .bg(theme.muted.opacity(0.35))
            .track_focus(&self.focus)
            .key_context("FlowCanvas")
            .on_action(cx.listener(|this, _: &DeleteSelection, window, cx| {
                this.delete_selection(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectAllBlocks, window, cx| this.select_all(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CopyBlocks, _, cx| this.copy_selection(cx)))
            .on_action(cx.listener(|this, _: &PasteBlocks, window, cx| this.paste(window, cx)))
            .on_action(
                cx.listener(|this, _: &DuplicateBlocks, window, cx| this.duplicate(window, cx)),
            )
            .on_action(cx.listener(|this, _: &UndoFlowEdit, window, cx| this.undo(window, cx)))
            .on_action(cx.listener(|this, _: &RedoFlowEdit, window, cx| this.redo(window, cx)))
            .on_action(cx.listener(|this, _: &AddBlock, window, cx| {
                let at = this.add_position();
                this.open_picker(at, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_step(true, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_step(false, cx)))
            .on_action(cx.listener(|this, _: &ZoomToFit, window, cx| this.zoom_to_fit(window, cx)))
            .on_action(cx.listener(|this, _: &ArrangeBlocks, window, cx| this.arrange(window, cx)))
            .on_action(cx.listener(|this, _: &StopFlow, window, cx| this.stop(window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        if this.picker.take().is_none() {
                            this.set_selection(Vec::new(), window, cx);
                        }
                        cx.notify();
                    }
                    // Backspace deletes through the rebindable action; the
                    // Delete key of full keyboards does too.
                    "delete" if this.picker.is_none() => this.delete_selection(window, cx),
                    "left" | "right" | "up" | "down" if !this.selection.is_empty() => {
                        let step = if event.keystroke.modifiers.shift {
                            32.
                        } else {
                            8.
                        };
                        let (dx, dy) = match event.keystroke.key.as_str() {
                            "left" => (-step, 0.),
                            "right" => (step, 0.),
                            "up" => (0., -step),
                            _ => (0., step),
                        };
                        let selection = this.selection.clone();
                        // Repeated presses are one edit to undo.
                        this.edit(
                            Some("nudge".to_owned()),
                            |flow| {
                                for id in &selection {
                                    if let Some(block) = flow.block_mut(id) {
                                        block.x += dx;
                                        block.y += dy;
                                    }
                                }
                            },
                            cx,
                        );
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.press(event, None, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.drag = Some(Drag::Pan {
                        last: event.position,
                    });
                }),
            )
            // A click quicker than a frame is released before the listeners
            // that follow a drag exist.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.release(event.position, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.release(event.position, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let position = this.canvas_position(event.position);
                    // The canvas takes focus once the press is handled, so
                    // the picker's search takes it after that.
                    cx.defer_in(window, move |this, window, cx| {
                        this.open_picker(position, None, window, cx);
                    });
                }),
            )
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_mouse_move(
                cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.hover_at(event.position, cx)
                }),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.hover.take().is_some() {
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, window, cx| {
                            let _ = entity.update(cx, |this, _| {
                                let resized = this.view.size != bounds.size;
                                this.view = bounds;
                                if !this.fitted && bounds.size.width > px(0.) {
                                    this.fitted = true;
                                    this.fit_on_open();
                                    // Changes made while drawing show in the next frame.
                                    window.request_animation_frame();
                                } else if resized {
                                    // What was culled for the old size may now be in view.
                                    window.request_animation_frame();
                                }

                                // Bring a block into view once the canvas has
                                // its size, after any drag that holds it.
                                if this.drag.is_none()
                                    && bounds.size.width > px(0.)
                                    && let Some(id) = this.pending_reveal.take()
                                    && this.reveal_now(&id)
                                {
                                    window.request_animation_frame();
                                }
                            });

                            window.insert_hitbox(bounds, HitboxBehavior::Normal)
                        }
                    },
                    move |bounds, hitbox, window, _| {
                        paint_grid(bounds, viewport, rem, grid_color, window);

                        // A pinch is handled where a scroll would be. Unlike
                        // `on_pinch`, this also works right after typing,
                        // before the pointer has moved.
                        let pinched = entity.clone();
                        window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window)
                            {
                                let _ = pinched.update(cx, |this, cx| this.pinch(event, cx));
                            }
                        });

                        for (curves, color, width) in &wires {
                            paint_curves(
                                bounds.origin,
                                curves,
                                viewport,
                                rem,
                                *color,
                                *width,
                                false,
                                window,
                            );
                        }
                        if let Some(pending) = &pending {
                            // Like Postman, a connection that is not joined
                            // to anything yet is dashed and ends in a ring
                            // at the pointer.
                            paint_curves(
                                bounds.origin,
                                std::slice::from_ref(&pending.curve),
                                viewport,
                                rem,
                                pending_color,
                                2.,
                                pending.kind == WireEnd::Pointer,
                                window,
                            );
                            paint_wire_end(
                                bounds.origin + viewport.to_view(pending.end, rem),
                                scale,
                                pending_color,
                                if pending.kind == WireEnd::Pointer {
                                    transparent_black()
                                } else {
                                    pending_color
                                },
                                window,
                            );
                        }
                        if let Some(area) = marquee {
                            let corner = viewport.to_view(area.origin, rem);
                            let size =
                                size(px(area.size.width * scale), px(area.size.height * scale));
                            window.paint_quad(quad(
                                Bounds {
                                    origin: bounds.origin + corner,
                                    size,
                                },
                                px(2.),
                                marquee_color.opacity(0.08),
                                px(1.),
                                marquee_color.opacity(0.7),
                                BorderStyle::Solid,
                            ));
                        }

                        // While dragging, follow the pointer outside the
                        // canvas and over blocks too.
                        if dragging {
                            let moving = entity.clone();
                            window.on_mouse_event(
                                move |event: &MouseMoveEvent, phase, window, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        let _ = moving.update(cx, |this, cx| {
                                            // The release was missed, such as
                                            // outside the window.
                                            if event.pressed_button.is_none() {
                                                this.release(event.position, window, cx)
                                            } else {
                                                this.drag_to(event.position, window, cx)
                                            }
                                        });
                                    }
                                },
                            );
                            let released = entity.clone();
                            window.on_mouse_event(
                                move |event: &MouseUpEvent, phase, window, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        let _ = released.update(cx, |this, cx| {
                                            this.release(event.position, window, cx)
                                        });
                                    }
                                },
                            );
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .child(Zoom::new(
                self.rem * self.viewport.zoom,
                div().absolute().inset_0().children(blocks),
            ))
            // The connections of what is selected or under the pointer are
            // drawn over the blocks too, so they can be followed past them.
            .when(!emphasized.is_empty(), |this| {
                this.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            for (curves, color, width) in &emphasized {
                                paint_curves(
                                    bounds.origin,
                                    curves,
                                    viewport,
                                    rem,
                                    *color,
                                    *width,
                                    false,
                                    window,
                                );
                            }
                        },
                    )
                    .absolute()
                    .inset_0()
                    .size_full(),
                )
            })
            .when(self.flow.blocks.is_empty(), |this| {
                this.child(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("This flow has no blocks")
                        .child("Right-click the canvas or choose Block to add one"),
                )
            })
            .when_some(self.picker.as_ref(), |this, picker| {
                this.child(self.render_picker(picker, window, cx))
            })
            .into_any_element()
    }

    /// Show the whole flow the first time the canvas has a size.
    fn fit_on_open(&mut self) {
        if let Some(content) = geometry::union(self.layouts.values().map(|layout| layout.bounds)) {
            self.viewport = geometry::Viewport::fit(content, self.view.size, self.rem);
        }
    }

    /// The curves of every connection in view with the color and width to
    /// draw them, and those to draw again over the blocks: the connections
    /// of what is selected or under the pointer. After a run, connections
    /// that carried data are colored, red for failures.
    fn wire_paths(&self, cx: &App) -> (Vec<WirePaint>, Vec<WirePaint>) {
        let theme = cx.theme();
        let visible = (self.view.size.width > px(0.))
            .then(|| self.viewport.visible(self.view.size, self.rem));
        let hovered_block = match &self.hover {
            Some(Hover::Block(block)) => Some(block),
            _ => None,
        };
        let mut wires = Vec::new();
        let mut emphasized = Vec::new();

        for connection in &self.flow.connections {
            // A connection being pulled off its input is drawn to the pointer instead.
            if matches!(&self.drag, Some(Drag::Connect { detached: Some(detached), .. }) if detached == connection)
            {
                continue;
            }
            let Some(curves) = self.wire(connection) else {
                continue;
            };
            // A curve stays within its control points.
            let reach = geometry::union(curves.iter().flatten().map(|corner| Bounds {
                origin: point(corner.x - 4., corner.y - 4.),
                size: size(8., 8.),
            }));
            if let (Some(visible), Some(reach)) = (&visible, &reach)
                && !geometry::intersects(visible, reach)
            {
                continue;
            }

            let carried = self
                .run
                .blocks
                .get(&connection.from)
                .is_some_and(|status| status.sent(&connection.output));
            let (color, width) = if carried && connection.output == "fail" {
                (theme.danger, 2.)
            } else if carried {
                (theme.success.opacity(0.5), 2.)
            } else {
                (theme.muted_foreground.opacity(0.55), 1.5)
            };

            let selected = self.selected_connection.as_ref() == Some(connection);
            let touches = |block: &String| *block == connection.from || *block == connection.to;
            let highlighted = selected
                || self.hover == Some(Hover::Connection(connection.clone()))
                || self.selection.iter().any(touches)
                || hovered_block.is_some_and(touches);

            if highlighted {
                // Faint over the blocks, so text it crosses stays readable.
                emphasized.push((curves.clone(), theme.ring.opacity(0.4), 1.5));
                wires.push((curves, theme.ring, if selected { 3. } else { 2. }));
            } else {
                wires.push((curves, color, width));
            }
        }

        (wires, emphasized)
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let running = self.run.running();
        let focus = self.focus.clone();

        h_flex()
            .debug_selector(|| "flow-toolbar".into())
            .flex_none()
            .h(rems(2.75))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(if running {
                Button::new("flow-stop")
                    .debug_selector(|| "flow-stop".into())
                    .small()
                    .danger()
                    .icon(Icon::default().path("icons/square.svg"))
                    .label("Stop")
                    .on_click(cx.listener(|this, _, window, cx| this.stop(window, cx)))
                    .into_any_element()
            } else {
                Button::new("flow-run")
                    .debug_selector(|| "flow-run".into())
                    .small()
                    .primary()
                    .icon(Icon::default().path("icons/play.svg"))
                    .label("Run")
                    .tooltip_with_action("Run the flow", &SendRequest, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, window, cx| this.run(window, cx)))
                    .into_any_element()
            })
            .child(
                Button::new("flow-add-block")
                    .debug_selector(|| "flow-add-block".into())
                    .small()
                    .ghost()
                    .icon(IconName::Plus)
                    .label("Block")
                    .tooltip_with_action("Add a block", &AddBlock, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let center = this.view_center();
                        this.open_picker(center, None, window, cx);
                    })),
            )
            .child(div().w_px().h_4().mx_1().bg(theme.border))
            .child(
                Button::new("flow-undo")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/undo-2.svg"))
                    .accessibility_label("Undo")
                    .disabled(!self.history.can_undo())
                    .tooltip_with_action("Undo", &UndoFlowEdit, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
            )
            .child(
                Button::new("flow-redo")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/redo-2.svg"))
                    .accessibility_label("Redo")
                    .disabled(!self.history.can_redo())
                    .tooltip_with_action("Redo", &RedoFlowEdit, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, window, cx| this.redo(window, cx))),
            )
            .child(
                Button::new("flow-arrange")
                    .debug_selector(|| "flow-arrange".into())
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/layout-dashboard.svg"))
                    .accessibility_label("Arrange blocks")
                    .tooltip_with_action(
                        "Arrange blocks left to right",
                        &ArrangeBlocks,
                        Some("FlowCanvas"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.arrange(window, cx))),
            )
            .child(div().flex_1())
            .when_some(
                self.run.summary.as_ref().filter(|_| !running),
                |this, summary| {
                    let failed = summary.failures > 0
                        || summary.stopped.is_some()
                        || self.run.failed_requests > 0;
                    this.child(
                        div()
                            .debug_selector(|| "flow-run-summary".into())
                            // Gives way to the buttons in a narrow window.
                            .min_w_0()
                            .truncate()
                            .px_1()
                            .text_xs()
                            .text_color(if failed {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(format!(
                                "{} · {} run{}{}",
                                super::run::format_duration(summary.elapsed),
                                summary.block_runs,
                                if summary.block_runs == 1 { "" } else { "s" },
                                super::run::failures(summary.failures, self.run.failed_requests),
                            )),
                    )
                },
            )
            .child(
                Button::new("flow-zoom-out")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/zoom-out.svg"))
                    .accessibility_label("Zoom out")
                    .tooltip_with_action("Zoom out", &ZoomOut, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_step(false, cx))),
            )
            .child(
                Button::new("flow-zoom-reset")
                    .debug_selector(|| "flow-zoom".into())
                    .small()
                    .ghost()
                    .label(format!("{:.0}%", self.viewport.zoom * 100.))
                    .tooltip("Zoom to 100%")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_to(1., cx))),
            )
            .child(
                Button::new("flow-zoom-in")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/zoom-in.svg"))
                    .accessibility_label("Zoom in")
                    .tooltip_with_action("Zoom in", &ZoomIn, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_step(true, cx))),
            )
            .child(
                Button::new("flow-fit")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/scan.svg"))
                    .accessibility_label("Zoom to fit")
                    .tooltip_with_action("Show the whole flow", &ZoomToFit, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, window, cx| this.zoom_to_fit(window, cx))),
            )
            .child(
                Button::new("flow-log")
                    .debug_selector(|| "flow-log-toggle".into())
                    .small()
                    .ghost()
                    .selected(self.log_open)
                    .icon(Icon::default().path("icons/panel-bottom.svg"))
                    .accessibility_label("Run log")
                    .tooltip("Show or hide the run log")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.log_open = !this.log_open;
                        window.focus(&focus, cx);
                        cx.notify();
                    })),
            )
    }
}

/// Dots every few canvas pixels, fewer when zoomed out so there are never
/// too many to draw.
fn paint_grid(
    bounds: Bounds<Pixels>,
    viewport: geometry::Viewport,
    rem: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let scale = viewport.scale(rem);
    let mut step = GRID;
    while step * scale < 24. {
        step *= 2.;
    }

    let dot = px((1.5 * scale.max(0.75)).min(2.));
    let first_x = (viewport.origin.x / step).ceil() * step;
    let first_y = (viewport.origin.y / step).ceil() * step;
    let visible = viewport.visible(bounds.size, rem);

    let mut y = first_y;
    while y < visible.origin.y + visible.size.height {
        let mut x = first_x;
        while x < visible.origin.x + visible.size.width {
            let at = bounds.origin + viewport.to_view(point(x, y), rem);
            window.paint_quad(fill(Bounds::new(at, size(dot, dot)), color));
            x += step;
        }
        y += step;
    }
}

/// Paint a connection's curves as one line.
#[allow(clippy::too_many_arguments)]
fn paint_curves(
    origin: Point<Pixels>,
    curves: &[[Point<f32>; 4]],
    viewport: geometry::Viewport,
    rem: Pixels,
    color: Hsla,
    width: f32,
    dashed: bool,
    window: &mut Window,
) {
    let Some(first) = curves.first() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    if dashed {
        path = path.dash_array(&[px(6.), px(4.)]);
    }
    path.move_to(origin + viewport.to_view(first[0], rem));
    for curve in curves {
        let [_, control, other, end] = curve.map(|point| origin + viewport.to_view(point, rem));
        path.cubic_bezier_to(end, control, other);
    }

    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The end of a connection being drawn, the size of a port's dot: a ring at
/// the pointer, or a dot where it joins a port or the block picker.
fn paint_wire_end(center: Point<Pixels>, scale: f32, color: Hsla, fill: Hsla, window: &mut Window) {
    let diameter = px((geometry::PORT * scale).max(6.));

    window.paint_quad(quad(
        Bounds::centered_at(center, size(diameter, diameter)),
        diameter / 2.,
        fill,
        px(2.),
        color,
        BorderStyle::Solid,
    ));
}
