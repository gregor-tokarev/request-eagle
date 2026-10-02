use std::{collections::HashMap, path::PathBuf, rc::Rc};

use environment::EnvironmentSessions;
use flow::{Block, BlockKind, BlockType, Connection, Flow};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::HttpRequest;

use super::{
    editing,
    geometry::{self, Viewport},
    history::History,
    inspector::Inspector,
    picker::Picker,
    run::RunState,
};
use crate::{Environments, RequestLocation};

/// A saved HTTP request that flows can send.
#[derive(Clone)]
pub struct FlowRequest {
    pub id: String,
    pub name: SharedString,
    /// The collection and folders that hold it, such as `API › Users`.
    pub location: SharedString,
    /// The directory of its collection.
    pub collection: PathBuf,
    pub request: HttpRequest,
}

/// Where a flow finds the saved requests its blocks send. The workspace
/// reads them from the collections sidebar.
pub trait FlowRequests {
    /// Every saved HTTP request, in the sidebar's order.
    fn all(&self, cx: &App) -> Vec<FlowRequest>;

    fn find(&self, id: &str, cx: &App) -> Option<FlowRequest>;
}

/// An input or output of a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PortRef {
    pub block: String,
    pub port: String,
    pub output: bool,
}

/// What an HTTP Request block shows of its request.
pub(super) struct RequestInfo {
    pub method: &'static str,
    pub name: SharedString,
    pub variables: Vec<SharedString>,
}

/// What the canvas draws for a block this frame.
pub(super) struct Layout {
    pub inputs: Vec<SharedString>,
    pub outputs: Vec<SharedString>,
    pub bounds: Bounds<f32>,
    /// The method of an HTTP Request block's request, and its name.
    pub request: Option<(&'static str, SharedString)>,
}

impl Layout {
    pub fn input_position(&self, index: usize) -> Point<f32> {
        point(
            self.bounds.origin.x,
            self.bounds.origin.y + geometry::port_offset(index),
        )
    }

    pub fn output_position(&self, index: usize) -> Point<f32> {
        point(
            self.bounds.origin.x + self.bounds.size.width,
            self.bounds.origin.y + geometry::port_offset(index),
        )
    }
}

/// A pointer drag on the canvas.
pub(super) enum Drag {
    Pan {
        last: Point<Pixels>,
    },
    Move {
        start: Point<f32>,
        origins: Vec<(String, Point<f32>)>,
        moved: bool,
    },
    /// A connection being drawn from a port to the pointer.
    Connect {
        from: PortRef,
        pointer: Point<f32>,
        /// The connection pulled off an input, which the drag replaces.
        detached: Option<Connection>,
    },
    Select {
        start: Point<f32>,
        end: Point<f32>,
        /// What was selected before, which a Shift-drag adds to.
        kept: Vec<String>,
    },
}

/// A flow's canvas, editing its blocks and connections, and its runs.
pub struct FlowEditor {
    pub location: RequestLocation,
    pub(super) flow: Flow,
    saved: Flow,
    pub(super) viewport: Viewport,
    pub(super) selection: Vec<String>,
    pub(super) selected_connection: Option<Connection>,
    pub(super) drag: Option<Drag>,
    /// The canvas's bounds in the window, as last drawn.
    pub(super) view: Bounds<Pixels>,
    pub(super) rem: Pixels,
    pub(super) layouts: HashMap<String, Layout>,
    /// The requests HTTP Request blocks send, looked up once rather than on
    /// every frame. Cleared whenever the canvas is focused or clicked, so
    /// edits made elsewhere show up.
    pub(super) request_info: HashMap<String, Option<RequestInfo>>,
    pub(super) history: History,
    pub(super) picker: Option<Picker>,
    pub(super) inspector: Option<Inspector>,
    pub(super) run: RunState,
    pub(super) requests: Rc<dyn FlowRequests>,
    pub(super) sessions: EnvironmentSessions,
    pub(super) environments: Option<Entity<Environments>>,
    pub(super) focus: FocusHandle,
    pub(super) log_open: bool,
    pub(super) log_scroll: UniformListScrollHandle,
    /// The view fits the flow once its size is known.
    pub(super) fitted: bool,
}

impl FlowEditor {
    pub fn new(
        location: RequestLocation,
        flow: Flow,
        requests: Rc<dyn FlowRequests>,
        sessions: EnvironmentSessions,
        environments: Option<Entity<Environments>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            location,
            saved: flow.clone(),
            flow,
            viewport: Viewport::default(),
            selection: Vec::new(),
            selected_connection: None,
            drag: None,
            view: Bounds::default(),
            rem: px(16.),
            layouts: HashMap::new(),
            request_info: HashMap::new(),
            history: History::default(),
            picker: None,
            inspector: None,
            run: RunState::default(),
            requests,
            sessions,
            environments,
            focus: cx.focus_handle(),
            log_open: false,
            log_scroll: UniformListScrollHandle::new(),
            fitted: false,
        }
    }

    pub fn flow(&self) -> &Flow {
        &self.flow
    }

    pub fn is_dirty(&self) -> bool {
        self.flow != self.saved
    }

    /// Show changes kept from the last session over the saved flow.
    pub fn restore_draft(&mut self, flow: Flow, cx: &mut Context<Self>) {
        self.flow = flow;
        cx.notify();
    }

    pub fn mark_saved(&mut self, flow: Flow, cx: &mut Context<Self>) {
        self.saved = flow;
        cx.notify();
    }

    /// Follow a rename or move made in the sidebar.
    pub fn set_location(&mut self, location: RequestLocation, cx: &mut Context<Self>) {
        self.location = location;
        cx.notify();
    }

    /// Focus the canvas, so its shortcuts apply.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_info.clear();
        if self.picker.is_none()
            && self
                .inspector
                .as_ref()
                .is_none_or(|inspector| !inspector.contains_focus(window, cx))
        {
            window.focus(&self.focus, cx);
        }
    }

    /// Why the flow cannot be saved as it is, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        self.flow.check()
    }

    /// Change the flow, remembering it as it was for undo. Changes with the
    /// same `key` join into one edit.
    pub(super) fn edit(
        &mut self,
        key: Option<String>,
        change: impl FnOnce(&mut Flow),
        cx: &mut Context<Self>,
    ) {
        self.history.record(&self.flow, key);
        change(&mut self.flow);
        cx.notify();
    }

    /// The inputs and outputs of each block and where it is, for this frame.
    pub(super) fn update_layouts(&mut self, cx: &App) {
        let mut layouts = HashMap::with_capacity(self.flow.blocks.len());

        for block in &self.flow.blocks {
            if let BlockKind::HttpRequest { request: id } = &block.kind
                && !self.request_info.contains_key(id)
            {
                let info = self.requests.find(id, cx).map(|saved| RequestInfo {
                    method: saved.request.method.as_str(),
                    name: saved.name,
                    variables: flow::request_variables(&saved.request)
                        .into_iter()
                        .map(SharedString::from)
                        .collect(),
                });
                self.request_info.insert(id.clone(), info);
            }
        }

        for block in &self.flow.blocks {
            let mut inputs: Vec<SharedString> = block
                .kind
                .inputs()
                .into_iter()
                .map(|name| SharedString::from(name.into_owned()))
                .collect();
            let mut request = None;

            if let BlockKind::HttpRequest { request: id } = &block.kind {
                if let Some(Some(info)) = self.request_info.get(id) {
                    inputs.extend(info.variables.iter().cloned());
                    request = Some((info.method, info.name.clone()));
                }

                // A connected input stays while its variable is out of the
                // request, so the connection can be seen and removed.
                for connection in &self.flow.connections {
                    if connection.to == block.id
                        && !inputs.iter().any(|input| *input == connection.input)
                    {
                        inputs.push(connection.input.clone().into());
                    }
                }
            }

            let outputs: Vec<SharedString> = block
                .kind
                .outputs()
                .into_iter()
                .map(|name| SharedString::from(name.into_owned()))
                .collect();
            let rows = inputs.len().max(outputs.len());

            layouts.insert(
                block.id.clone(),
                Layout {
                    bounds: Bounds {
                        origin: point(block.x, block.y),
                        size: geometry::block_size(&block.kind, rows),
                    },
                    inputs,
                    outputs,
                    request,
                },
            );
        }

        self.layouts = layouts;
    }

    /// The port nearest to a canvas position, within reach of the pointer.
    pub(super) fn port_at(&self, position: Point<f32>, outputs: bool) -> Option<PortRef> {
        let reach = geometry::GRAB / self.viewport.scale(self.rem);
        let mut nearest: Option<(f32, PortRef)> = None;

        for block in &self.flow.blocks {
            let Some(layout) = self.layouts.get(&block.id) else {
                continue;
            };
            let ports = if outputs {
                &layout.outputs
            } else {
                &layout.inputs
            };

            for (index, port) in ports.iter().enumerate() {
                let at = if outputs {
                    layout.output_position(index)
                } else {
                    layout.input_position(index)
                };
                let distance = ((at.x - position.x).powi(2) + (at.y - position.y).powi(2)).sqrt();
                if distance <= reach && nearest.as_ref().is_none_or(|(best, _)| distance < *best) {
                    nearest = Some((
                        distance,
                        PortRef {
                            block: block.id.clone(),
                            port: port.to_string(),
                            output: outputs,
                        },
                    ));
                }
            }
        }

        nearest.map(|(_, port)| port)
    }

    /// The port a connection being drawn would join, under the pointer.
    pub(super) fn connecting_port(&self) -> Option<PortRef> {
        match &self.drag {
            Some(Drag::Connect { from, pointer, .. }) => self.port_at(*pointer, !from.output),
            _ => None,
        }
    }

    /// The topmost block at a canvas position.
    pub(super) fn block_at(&self, position: Point<f32>) -> Option<String> {
        self.flow.blocks.iter().rev().find_map(|block| {
            let bounds = self.layouts.get(&block.id)?.bounds;
            (position.x >= bounds.origin.x
                && position.x <= bounds.origin.x + bounds.size.width
                && position.y >= bounds.origin.y
                && position.y <= bounds.origin.y + bounds.size.height)
                .then(|| block.id.clone())
        })
    }

    /// The connection nearest to a canvas position, within reach of the pointer.
    pub(super) fn connection_at(&self, position: Point<f32>) -> Option<Connection> {
        let reach = geometry::GRAB / 2. / self.viewport.scale(self.rem);

        self.flow
            .connections
            .iter()
            .filter_map(|connection| {
                let curve = self.wire(connection)?;
                let distance = geometry::distance_to_wire(position, &curve);
                (distance <= reach).then_some((distance, connection))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, connection)| connection.clone())
    }

    /// The curve of a connection between the ports it joins.
    pub(super) fn wire(&self, connection: &Connection) -> Option<[Point<f32>; 4]> {
        let from = self.layouts.get(&connection.from)?;
        let to = self.layouts.get(&connection.to)?;
        let output = from
            .outputs
            .iter()
            .position(|output| *output == connection.output)?;
        let input = to
            .inputs
            .iter()
            .position(|input| *input == connection.input)?;

        Some(geometry::wire(
            from.output_position(output),
            to.input_position(input),
        ))
    }

    pub(super) fn select(
        &mut self,
        id: &str,
        additive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_connection = None;
        self.history.seal();

        if additive {
            if let Some(index) = self.selection.iter().position(|selected| selected == id) {
                self.selection.remove(index);
            } else {
                self.selection.push(id.to_owned());
            }
        } else if !self.selection.iter().any(|selected| selected == id) {
            self.selection = vec![id.to_owned()];
        }

        self.sync_inspector(window, cx);
        cx.notify();
    }

    pub(super) fn set_selection(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection = ids;
        self.history.seal();
        self.selected_connection = None;
        self.sync_inspector(window, cx);
        cx.notify();
    }

    pub(super) fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self
            .flow
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect();
        self.set_selection(ids, window, cx);
    }

    /// Add a block at a canvas position, connecting it to `from` when a
    /// connection was drawn to an empty spot. Returns the block's ID.
    pub(super) fn add_block(
        &mut self,
        kind: BlockKind,
        position: Point<f32>,
        from: Option<PortRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let id = self.flow.next_block_id();
        let loops = matches!(kind, BlockKind::For | BlockKind::Repeat);
        let block = Block {
            id: id.clone(),
            title: None,
            x: position.x,
            y: position.y,
            kind,
        };

        self.edit(
            None,
            |flow| {
                // A drawn connection reaches the new block's first matching port.
                if let Some(from) = from {
                    let connection = if from.output {
                        block.kind.inputs().first().map(|input| Connection {
                            from: from.block,
                            output: from.port,
                            to: block.id.clone(),
                            input: input.to_string(),
                        })
                    } else {
                        block.kind.outputs().first().map(|output| Connection {
                            from: block.id.clone(),
                            output: output.to_string(),
                            to: from.block,
                            input: from.port,
                        })
                    };
                    flow.blocks.push(block);
                    if let Some(connection) = connection {
                        flow.connect(connection);
                    }
                } else {
                    flow.blocks.push(block);
                }

                // Like Postman, a new loop comes with the Collect block that
                // ends it.
                if loops {
                    let collect = flow.next_block_id();
                    flow.blocks.push(Block {
                        id: collect,
                        title: None,
                        x: position.x + 520.,
                        y: position.y,
                        kind: BlockType::Collect.block_kind(),
                    });
                }
            },
            cx,
        );

        self.set_selection(vec![id.clone()], window, cx);
        id
    }

    /// Connect an output to an input, replacing the input's connection.
    pub(super) fn connect(&mut self, a: PortRef, b: PortRef, cx: &mut Context<Self>) {
        let Some(connection) = connection_between(a, b) else {
            return;
        };
        if self.flow.connections.contains(&connection) {
            return;
        }

        self.edit(None, |flow| flow.connect(connection), cx);
    }

    pub(super) fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(connection) = self.selected_connection.take() {
            self.edit(
                None,
                |flow| flow.connections.retain(|existing| *existing != connection),
                cx,
            );
            return;
        }

        if self.selection.is_empty() {
            return;
        }

        let selection = std::mem::take(&mut self.selection);
        self.edit(None, |flow| flow.remove_blocks(&selection), cx);
        self.sync_inspector(window, cx);
    }

    pub(super) fn copy_selection(&self, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            return;
        }

        let copied = editing::copy(&self.flow, &self.selection);
        cx.write_to_clipboard(ClipboardItem::new_string(editing::to_clipboard(&copied)));
    }

    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(copied) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| editing::from_clipboard(&text))
        else {
            return;
        };

        self.insert_copy(copied, window, cx);
    }

    pub(super) fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            return;
        }

        let copied = editing::copy(&self.flow, &self.selection);
        self.insert_copy(copied, window, cx);
    }

    fn insert_copy(&mut self, copied: Flow, window: &mut Window, cx: &mut Context<Self>) {
        let mut pasted = Vec::new();
        self.edit(
            None,
            |flow| pasted = editing::paste(flow, copied, point(32., 32.)),
            cx,
        );
        self.set_selection(pasted, window, cx);
    }

    pub(super) fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.undo(&mut self.flow) {
            self.after_history(window, cx);
        }
    }

    pub(super) fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.redo(&mut self.flow) {
            self.after_history(window, cx);
        }
    }

    fn after_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection
            .retain(|id| self.flow.blocks.iter().any(|block| block.id == *id));
        self.selected_connection = None;
        // The settings shown may have changed.
        self.inspector = None;
        self.sync_inspector(window, cx);
        cx.notify();
    }

    pub(super) fn arrange(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layouts = &self.layouts;
        let sizes: HashMap<String, Size<f32>> = layouts
            .iter()
            .map(|(id, layout)| (id.clone(), layout.bounds.size))
            .collect();

        self.edit(
            None,
            |flow| {
                editing::arrange(flow, |block| {
                    sizes
                        .get(&block.id)
                        .copied()
                        .unwrap_or_else(|| geometry::block_size(&block.kind, 1))
                })
            },
            cx,
        );
        self.update_layouts(cx);
        self.zoom_to_fit(window, cx);
    }

    pub(super) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = point(self.view.size.width / 2., self.view.size.height / 2.);
        let zoom = self.viewport.zoom * factor;
        self.viewport.zoom_around(zoom, center, self.rem);
        cx.notify();
    }

    pub(super) fn zoom_to_fit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let content = geometry::union(self.layouts.values().map(|layout| layout.bounds));
        if let Some(content) = content
            && self.view.size.width > px(0.)
        {
            self.viewport = Viewport::fit(content, self.view.size, self.rem);
        }
        cx.notify();
    }

    /// Move the view so a block is in it, unless it already is.
    pub(super) fn reveal(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(layout) = self.layouts.get(id) else {
            return;
        };
        let visible = self.viewport.visible(self.view.size, self.rem);
        let bounds = layout.bounds;
        let inside = bounds.origin.x >= visible.origin.x
            && bounds.origin.y >= visible.origin.y
            && bounds.origin.x + bounds.size.width <= visible.origin.x + visible.size.width
            && bounds.origin.y + bounds.size.height <= visible.origin.y + visible.size.height;

        if !inside {
            self.viewport.origin = point(
                bounds.origin.x + bounds.size.width / 2. - visible.size.width / 2.,
                bounds.origin.y + bounds.size.height / 2. - visible.size.height / 2.,
            );
            cx.notify();
        }
    }

    /// The canvas position at the middle of the view, where blocks added
    /// from the toolbar go.
    pub(super) fn view_center(&self) -> Point<f32> {
        self.viewport.to_canvas(
            point(self.view.size.width / 2., self.view.size.height / 3.),
            self.rem,
        )
    }
}

/// The connection from an output to an input, given in either order,
/// unless the two ports cannot be connected.
pub(super) fn connection_between(a: PortRef, b: PortRef) -> Option<Connection> {
    let (output, input) = if a.output { (a, b) } else { (b, a) };

    (output.output != input.output && output.block != input.block).then_some(Connection {
        from: output.block,
        output: output.port,
        to: input.block,
        input: input.port,
    })
}

impl Render for FlowEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rem = window.rem_size();
        self.update_layouts(cx);

        v_flex()
            .debug_selector(|| "flow-editor".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_canvas(window, cx))
                    .children(self.render_inspector(cx)),
            )
            .when(self.log_open, |this| this.child(self.render_run_log(cx)))
    }
}

impl Focusable for FlowEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
