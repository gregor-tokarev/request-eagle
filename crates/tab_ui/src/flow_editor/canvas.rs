use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    FlowEditor,
    actions::*,
    editor::{Drag, PortRef, connection_between},
    geometry,
    picker::Picker,
    zoom::Zoom,
};
use crate::SendRequest;

/// Canvas pixels between the dots of the background grid.
const GRID: f32 = 24.;

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
                    .selection
                    .iter()
                    .filter_map(|id| {
                        let block = self.flow.block(id)?;
                        Some((id.clone(), point(block.x, block.y)))
                    })
                    .collect();
                self.drag = Some(Drag::Move {
                    start: position,
                    origins,
                    moved: false,
                });
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

    fn drag_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
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
            Some(Drag::Select { start, end, kept }) => {
                *end = pointer;
                let area = geometry::rectangle(*start, *end);
                let mut selection = kept.clone();
                for block in &self.flow.blocks {
                    if let Some(layout) = self.layouts.get(&block.id)
                        && geometry::intersects(&area, &layout.bounds)
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
        let pointer = self.canvas_position(position);

        match drag {
            Drag::Connect { from, detached, .. } => {
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
                    // A connection pulled off and dropped on the canvas is removed.
                    (None, Some(connection)) => self.edit(
                        None,
                        |flow| flow.connections.retain(|existing| *existing != connection),
                        cx,
                    ),
                    // Dropped on the canvas: choose a block to connect to.
                    (None, None) => self.open_picker(pointer, Some(from), window, cx),
                }
            }
            Drag::Move { moved: true, .. } => self.history.seal(),
            _ => {}
        }

        cx.notify();
    }

    /// The port a connection drawn from `from` joins when dropped at a
    /// position: a port near it, or the first free port of the block there.
    pub(super) fn drop_port(&self, from: &PortRef, position: Point<f32>) -> Option<PortRef> {
        self.port_at(position, !from.output).or_else(|| {
            self.block_at(position)
                .and_then(|block| self.free_port(&block, !from.output))
        })
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
        let delta = event.delta.pixel_delta(window.line_height());

        if event.modifiers.secondary() || event.modifiers.control {
            let factor = (f32::from(delta.y) * 0.004).exp();
            let zoom = self.viewport.zoom * factor;
            self.viewport
                .zoom_around(zoom, event.position - self.view.origin, self.rem);
        } else {
            self.viewport.pan(delta, self.rem);
        }

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
        let visible = (self.view.size.width > px(0.))
            .then(|| self.viewport.visible(self.view.size, self.rem));
        let blocks: Vec<AnyElement> = self
            .flow
            .blocks
            .iter()
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

        let wires = self.wire_paths(cx);
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
                let center = this.view_center();
                this.open_picker(center, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1.25, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(0.8, cx)))
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
                            });
                        }
                    },
                    move |bounds, _, window, _| {
                        paint_grid(bounds, viewport, rem, grid_color, window);

                        for (curve, color, width) in &wires {
                            paint_curve(
                                bounds.origin,
                                curve,
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
                            paint_curve(
                                bounds.origin,
                                &pending.curve,
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

    /// The curves of every connection with the color and width to draw it.
    fn wire_paths(&self, cx: &App) -> Vec<([Point<f32>; 4], Hsla, f32)> {
        let theme = cx.theme();
        let visible = (self.view.size.width > px(0.))
            .then(|| self.viewport.visible(self.view.size, self.rem));

        self.flow
            .connections
            .iter()
            // A connection being pulled off its input is drawn to the pointer instead.
            .filter(|connection| {
                !matches!(&self.drag, Some(Drag::Connect { detached: Some(detached), .. }) if detached == *connection)
            })
            .filter_map(|connection| {
                let curve = self.wire(connection)?;
                // A curve stays within its control points.
                let reach = geometry::union(curve.map(|corner| Bounds {
                    origin: point(corner.x - 4., corner.y - 4.),
                    size: size(8., 8.),
                }))?;
                if visible
                    .as_ref()
                    .is_some_and(|visible| !geometry::intersects(visible, &reach))
                {
                    return None;
                }

                let carried = self
                    .run
                    .blocks
                    .get(&connection.from)
                    .is_some_and(|status| status.sent(&connection.output));
                let (color, width) = if self.selected_connection.as_ref() == Some(connection) {
                    (theme.ring, 3.)
                } else if self.selection.contains(&connection.from)
                    || self.selection.contains(&connection.to)
                {
                    (theme.ring, 2.)
                } else if carried {
                    (theme.muted_foreground, 2.)
                } else {
                    (theme.muted_foreground.opacity(0.55), 1.5)
                };

                Some((curve, color, width))
            })
            .collect()
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
                    let failed = summary.failures > 0 || summary.stopped.is_some();
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
                                if summary.failures > 0 {
                                    format!(", {} failed", summary.failures)
                                } else {
                                    String::new()
                                }
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
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(0.8, cx))),
            )
            .child(
                Button::new("flow-zoom-reset")
                    .debug_selector(|| "flow-zoom".into())
                    .small()
                    .ghost()
                    .label(format!("{:.0}%", self.viewport.zoom * 100.))
                    .tooltip("Zoom to 100%")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let zoom = 1. / this.viewport.zoom;
                        this.zoom_by(zoom, cx);
                    })),
            )
            .child(
                Button::new("flow-zoom-in")
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/zoom-in.svg"))
                    .accessibility_label("Zoom in")
                    .tooltip_with_action("Zoom in", &ZoomIn, Some("FlowCanvas"))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))),
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

#[allow(clippy::too_many_arguments)]
fn paint_curve(
    origin: Point<Pixels>,
    curve: &[Point<f32>; 4],
    viewport: geometry::Viewport,
    rem: Pixels,
    color: Hsla,
    width: f32,
    dashed: bool,
    window: &mut Window,
) {
    let [start, first, second, end] = curve.map(|point| origin + viewport.to_view(point, rem));
    let mut path = PathBuilder::stroke(px(width));
    if dashed {
        path = path.dash_array(&[px(6.), px(4.)]);
    }
    path.move_to(start);
    path.cubic_bezier_to(end, first, second);

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
