use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

use collection::{Collections, FileEntry, SavedLocation};
use environment::EnvironmentSessions;
use flow::{Block, BlockKind, BlockType, Connection, Flow, SavedFlow};
use gpui_kit::component::{
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Auth, HttpRequest, Request};

use super::{
    blocks, editing,
    geometry::{self, Viewport},
    history::History,
    inspector::Inspector,
    picker::Picker,
    run::RunState,
};
use crate::{Environments, variables::VariableScope};

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
    /// What it sends when it inherits its collection's authorization.
    pub collection_auth: Auth,
}

impl FlowRequest {
    /// Every saved HTTP request, in the sidebar's order.
    pub(super) fn all(collections: &Collections) -> Vec<Self> {
        collections
            .registry()
            .collections()
            .iter()
            .flat_map(|collection| {
                collections
                    .requests_in(&collection.path)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(location, file)| Self::new(location, file, collection.auth()))
            })
            .collect()
    }

    pub(super) fn find(collections: &Collections, id: &str) -> Option<Self> {
        let (collection, file) = collections.registry().request_by_id(id)?;

        Self::new(collections.location(file)?, file, collection.auth())
    }

    fn new(location: SavedLocation, file: &FileEntry, collection_auth: &Auth) -> Option<Self> {
        let Request::Http(request) = &file.request else {
            return None;
        };

        let mut breadcrumb = location.collection_name();
        for folder in location.folders() {
            breadcrumb.push_str(" › ");
            breadcrumb.push_str(&folder);
        }

        Some(Self {
            id: location.id,
            name: location.name.into(),
            location: breadcrumb.into(),
            collection: location.collection,
            request: request.clone(),
            collection_auth: collection_auth.clone(),
        })
    }
}

/// An input or output of a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PortRef {
    pub block: String,
    pub port: String,
    pub output: bool,
}

/// What an HTTP Request block shows of its request.
#[derive(Clone)]
pub(super) struct RequestInfo {
    pub method: &'static str,
    pub name: SharedString,
    /// Its URL as saved, variables and all.
    pub path: SharedString,
    pub variables: Vec<SharedString>,
    /// The directory of its collection.
    pub collection: PathBuf,
}

/// The variables a collection's requests find a value for, from the
/// collection, the active environment and the session, and what they were
/// read from, to tell when they change.
struct FilledNames {
    revision: u64,
    active: Option<PathBuf>,
    versions: Vec<Option<SystemTime>>,
    names: Rc<HashSet<String>>,
}

/// What the canvas draws for a block this frame.
pub(super) struct Layout {
    pub inputs: Vec<SharedString>,
    pub outputs: Vec<SharedString>,
    pub bounds: Bounds<f32>,
    /// The request an HTTP Request block sends.
    pub request: Option<RequestInfo>,
    /// The variables of that request its collection, the active environment
    /// or the session fill, which an unconnected input sends.
    pub filled: Option<Rc<HashSet<String>>>,
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
    /// A Note's corner being dragged to resize it.
    Resize {
        block: String,
        start: Point<f32>,
        size: Size<f32>,
        resized: bool,
    },
}

/// What the pointer is over, whose connections the canvas highlights.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hover {
    Block(String),
    Connection(Connection),
}

/// A flow's canvas, editing its blocks and connections, and its runs.
pub struct FlowEditor {
    /// Where the flow is saved. Renaming a flow keeps its file.
    pub path: PathBuf,
    /// Tells the flow apart from another one saved at the same path later.
    pub id: SharedString,
    pub name: SharedString,
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
    /// The variables each collection's requests find a value for.
    filled_names: HashMap<PathBuf, FilledNames>,
    pub(super) history: History,
    pub(super) picker: Option<Picker>,
    pub(super) inspector: Option<Inspector>,
    pub(super) run: RunState,
    pub(super) collections: Entity<Collections>,
    pub(super) sessions: EnvironmentSessions,
    pub(super) environments: Option<Entity<Environments>>,
    pub(super) focus: FocusHandle,
    pub(super) log_open: bool,
    pub(super) log_scroll: UniformListScrollHandle,
    /// The view fits the flow once its size is known.
    pub(super) fitted: bool,
    pub(super) hover: Option<Hover>,
    /// Where the pointer last was over the canvas, in the window, where
    /// blocks added from the keyboard go.
    pub(super) pointer: Option<Point<Pixels>>,
    /// A block to bring into view once the canvas has its new size, such as
    /// one the inspector opened beside.
    pub(super) pending_reveal: Option<String>,
    /// Pans the canvas while a drag is held at its edge.
    pub(super) edge_pan: Option<Task<()>>,
    /// The run log shows only the selected block's entries.
    pub(super) log_filtered: bool,
    pub(super) inspector_split: Entity<ResizableState>,
    pub(super) log_split: Entity<ResizableState>,
    /// Redraws when another environment is chosen, which fills other
    /// variables.
    _environments: Option<Subscription>,
}

impl FlowEditor {
    pub fn new(
        saved: SavedFlow,
        collections: Entity<Collections>,
        sessions: EnvironmentSessions,
        environments: Option<Entity<Environments>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observed = environments
            .as_ref()
            .map(|environments| cx.observe(environments, |_, _, cx| cx.notify()));

        Self {
            path: saved.path,
            id: saved.id.into(),
            name: saved.name.into(),
            saved: saved.flow.clone(),
            flow: saved.flow,
            viewport: Viewport::default(),
            selection: Vec::new(),
            selected_connection: None,
            drag: None,
            view: Bounds::default(),
            rem: px(16.),
            layouts: HashMap::new(),
            request_info: HashMap::new(),
            filled_names: HashMap::new(),
            history: History::default(),
            picker: None,
            inspector: None,
            run: RunState::default(),
            collections,
            sessions,
            environments,
            focus: cx.focus_handle(),
            log_open: false,
            log_scroll: UniformListScrollHandle::new(),
            fitted: false,
            hover: None,
            pointer: None,
            pending_reveal: None,
            edge_pan: None,
            log_filtered: false,
            inspector_split: cx.new(|_| ResizableState::default()),
            log_split: cx.new(|_| ResizableState::default()),
            _environments: observed,
        }
    }

    pub fn flow(&self) -> &Flow {
        &self.flow
    }

    pub fn is_dirty(&self) -> bool {
        self.flow != self.saved
    }

    /// Show changes kept from the last session over the saved flow.
    /// Follow a rename made in the sidebar or the tab.
    pub fn set_name(&mut self, name: SharedString, cx: &mut Context<Self>) {
        self.name = name;
        cx.notify();
    }

    pub fn restore_draft(&mut self, flow: Flow, cx: &mut Context<Self>) {
        self.flow = flow;
        cx.notify();
    }

    pub fn mark_saved(&mut self, flow: Flow, cx: &mut Context<Self>) {
        self.saved = flow;
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

    /// Why the flow cannot be saved or run as it is, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        if let Some((_, error)) = self
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.errors.first())
        {
            return Err(format!("Fix the selected block's settings: {error}"));
        }

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
                let info = FlowRequest::find(self.collections.read(cx), id)
                    .map(|saved| self.describe_request(saved));
                self.request_info.insert(id.clone(), info);
            }
        }

        let collections: Vec<PathBuf> = self
            .request_info
            .values()
            .flatten()
            .map(|info| info.collection.clone())
            .collect();
        let filled: HashMap<PathBuf, Rc<HashSet<String>>> = collections
            .into_iter()
            .map(|collection| {
                let names = self.filled(&collection, cx);
                (collection, names)
            })
            .collect();

        let mut connected: HashMap<&str, Vec<&str>> = HashMap::new();
        for connection in &self.flow.connections {
            connected
                .entry(&connection.to)
                .or_default()
                .push(&connection.input);
        }

        for block in &self.flow.blocks {
            let mut inputs: Vec<SharedString> = block
                .kind
                .inputs()
                .into_iter()
                .map(|name| SharedString::from(name.into_owned()))
                .collect();
            let mut request = None;
            let mut names = None;

            if let BlockKind::HttpRequest { request: id } = &block.kind {
                // Send also fills a `{{send}}` variable, so it is drawn once.
                if let Some(Some(info)) = self.request_info.get(id) {
                    for variable in &info.variables {
                        if !inputs.contains(variable) {
                            inputs.push(variable.clone());
                        }
                    }
                    request = Some(info.clone());
                    names = filled.get(&info.collection).cloned();
                }

                // A connected input stays while its variable is out of the
                // request, so the connection can be seen and removed.
                for &input in connected.get(block.id.as_str()).into_iter().flatten() {
                    if !inputs.iter().any(|name| name == input) {
                        inputs.push(SharedString::from(input.to_owned()));
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
            let message = self
                .run
                .blocks
                .get(&block.id)
                .and_then(|status| status.last.as_ref())
                .is_some_and(|run| run.error.is_some() || run.notice.is_some());

            layouts.insert(
                block.id.clone(),
                Layout {
                    bounds: Bounds {
                        origin: point(block.x, block.y),
                        size: geometry::block_size(&block.kind, rows, message),
                    },
                    inputs,
                    outputs,
                    request,
                    filled: names,
                },
            );
        }

        self.layouts = layouts;
    }

    /// What an HTTP Request block shows of a saved request.
    fn describe_request(&self, saved: FlowRequest) -> RequestInfo {
        RequestInfo {
            method: saved.request.method.as_str(),
            path: SharedString::from(saved.request.path.clone()),
            variables: flow::request_variables(&saved.request, &saved.collection_auth)
                .into_iter()
                .map(SharedString::from)
                .collect(),
            collection: saved.collection,
            name: saved.name,
        }
    }

    /// The variables a collection's requests find a value for, read again
    /// only when the session, the active environment or their files change.
    fn filled(&mut self, collection: &Path, cx: &App) -> Rc<HashSet<String>> {
        let environment = collection.join("environment.toml");
        let scope = VariableScope {
            path: Some(environment.clone()),
            session: self.sessions.for_path(Some(&environment)),
            environments: self.environments.clone(),
            names: None,
        };
        let revision = scope.session.revision();
        let active = self
            .environments
            .as_ref()
            .and_then(|environments| environments.read(cx).active_path());
        let versions = scope.file_versions(cx);

        if let Some(known) = self.filled_names.get(collection)
            && known.revision == revision
            && known.active == active
            && known.versions == versions
        {
            return known.names.clone();
        }

        let names: Rc<HashSet<String>> = Rc::new(
            scope
                .values(cx)
                .map(|values| values.into_keys().collect())
                .unwrap_or_default(),
        );
        self.filled_names.insert(
            collection.to_owned(),
            FilledNames {
                revision,
                active,
                versions,
                names: names.clone(),
            },
        );
        names
    }

    /// The name a block goes by on the canvas and in the run log: its own
    /// title, the name of the request an HTTP Request block sends, or its
    /// type's name.
    pub(super) fn block_title(&self, block: &Block, cx: &App) -> SharedString {
        if let Some(title) = block
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            return SharedString::from(title.to_owned());
        }
        if let BlockKind::HttpRequest { request } = &block.kind {
            let name = match self.request_info.get(request) {
                Some(info) => info.as_ref().map(|info| info.name.clone()),
                None => {
                    FlowRequest::find(self.collections.read(cx), request).map(|saved| saved.name)
                }
            };
            if let Some(name) = name {
                return name;
            }
        }
        SharedString::from(block.title().to_owned())
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

    /// The port a connection being drawn would join if it was dropped now.
    pub(super) fn connecting_port(&self) -> Option<PortRef> {
        match &self.drag {
            Some(Drag::Connect { from, pointer, .. }) => self.drop_port(from, *pointer),
            _ => None,
        }
    }

    /// The topmost block at a canvas position. Notes lie behind the other
    /// blocks and are taken by their heading or border, so the inside of a
    /// frame works like the canvas: its connections can be chosen and areas
    /// selected there.
    pub(super) fn block_at(&self, position: Point<f32>) -> Option<String> {
        let at = |block: &&Block| {
            self.layouts.get(&block.id).is_some_and(|layout| {
                let bounds = layout.bounds;
                position.x >= bounds.origin.x
                    && position.x <= bounds.origin.x + bounds.size.width
                    && position.y >= bounds.origin.y
                    && position.y <= bounds.origin.y + bounds.size.height
            })
        };
        let note = |block: &&Block| matches!(block.kind, BlockKind::Note { .. });
        let grip = |block: &&Block| {
            self.layouts
                .get(&block.id)
                .is_some_and(|layout| self.note_grip(layout.bounds, position))
        };

        let blocks = self.flow.blocks.iter().rev();
        blocks
            .clone()
            .filter(|block| !note(block))
            .find(at)
            .or_else(|| blocks.filter(note).find(grip))
            .map(|block| block.id.clone())
    }

    /// Whether a position is on a Note's heading or border, or on the label
    /// it shows above itself when zoomed out.
    fn note_grip(&self, bounds: Bounds<f32>, position: Point<f32>) -> bool {
        let scale = self.viewport.scale(self.rem);
        let border = geometry::GRAB / scale;
        let label = if self.viewport.zoom < geometry::DETAIL_ZOOM {
            blocks::NOTE_LABEL * f32::from(self.rem) / geometry::BASE_REM / scale
        } else {
            0.
        };
        let (left, top) = (bounds.origin.x, bounds.origin.y);
        let (right, bottom) = (left + bounds.size.width, top + bounds.size.height);

        let near = position.x >= left - border
            && position.x <= right + border
            && position.y >= top - label - border
            && position.y <= bottom + border;
        near && (position.y <= top + geometry::NOTE_HEADING
            || position.x <= left + border
            || position.x >= right - border
            || position.y >= bottom - border)
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

    /// The curves of a connection between the ports it joins.
    pub(super) fn wire(&self, connection: &Connection) -> Option<Vec<[Point<f32>; 4]>> {
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

        let bottom = |layout: &Layout| layout.bounds.origin.y + layout.bounds.size.height;
        Some(geometry::route(
            from.output_position(output),
            to.input_position(input),
            (bottom(from), bottom(to)),
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
        let position = self.free_spot(&kind, position, cx);
        let collect_position = loops.then(|| {
            self.free_spot(
                &BlockType::Collect.block_kind(),
                point(position.x + 520., position.y),
                cx,
            )
        });
        let block = Block {
            id: id.clone(),
            title: None,
            x: position.x,
            y: position.y,
            kind,
        };
        let mut added = vec![id.clone()];

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
                if let Some(at) = collect_position {
                    let collect = flow.next_block_id();
                    added.push(collect.clone());
                    flow.blocks.push(Block {
                        id: collect,
                        title: None,
                        x: at.x,
                        y: at.y,
                        kind: BlockType::Collect.block_kind(),
                    });
                }
            },
            cx,
        );
        self.forget_runs(&added);

        self.set_selection(vec![id.clone()], window, cx);
        self.pending_reveal = Some(id.clone());
        id
    }

    /// Where a new block can go at or just below `position` without covering
    /// another block. Notes may be covered; they frame blocks.
    fn free_spot(&self, kind: &BlockKind, position: Point<f32>, cx: &App) -> Point<f32> {
        editing::free_spot(&self.taken(), self.drawn_size(kind, cx), position)
    }

    /// The size a block is drawn at before it has run, with a row for each
    /// variable of an HTTP Request block's request.
    fn drawn_size(&self, kind: &BlockKind, cx: &App) -> Size<f32> {
        let mut inputs: Vec<String> = kind
            .inputs()
            .into_iter()
            .map(|name| name.into_owned())
            .collect();
        if let BlockKind::HttpRequest { request } = kind
            && let Some(saved) = FlowRequest::find(self.collections.read(cx), request)
        {
            for variable in flow::request_variables(&saved.request, &saved.collection_auth) {
                if !inputs.contains(&variable) {
                    inputs.push(variable);
                }
            }
        }
        let rows = inputs.len().max(kind.outputs().len());
        geometry::block_size(kind, rows, false)
    }

    /// Where the blocks other than Notes are.
    fn taken(&self) -> Vec<Bounds<f32>> {
        self.flow
            .blocks
            .iter()
            .filter(|block| !matches!(block.kind, BlockKind::Note { .. }))
            .filter_map(|block| Some(self.layouts.get(&block.id)?.bounds))
            .collect()
    }

    /// New blocks can take the IDs of deleted ones; they have not run.
    fn forget_runs(&mut self, blocks: &[String]) {
        for block in blocks {
            self.run.blocks.remove(block);
        }
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
        // Copies go beside their originals, but not over another block.
        let group: Vec<Bounds<f32>> = copied
            .blocks
            .iter()
            .filter(|block| !matches!(block.kind, BlockKind::Note { .. }))
            .map(|block| Bounds {
                origin: point(block.x, block.y),
                size: self
                    .layouts
                    .get(&block.id)
                    .filter(|layout| layout.bounds.origin == point(block.x, block.y))
                    .map(|layout| layout.bounds.size)
                    .unwrap_or_else(|| self.drawn_size(&block.kind, cx)),
            })
            .collect();
        let offset = editing::free_offset(&self.taken(), &group, point(32., 32.));

        let mut pasted = Vec::new();
        self.edit(
            None,
            |flow| pasted = editing::paste(flow, copied, offset),
            cx,
        );
        self.forget_runs(&pasted);
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
        self.refresh_displays();
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
                        .unwrap_or_else(|| geometry::block_size(&block.kind, 1, false))
                })
            },
            cx,
        );
        self.update_layouts(cx);
        self.zoom_to_fit(window, cx);
    }

    /// Zoom around the middle of the view.
    pub(super) fn zoom_to(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let center = point(self.view.size.width / 2., self.view.size.height / 2.);
        self.viewport.zoom_around(zoom, center, self.rem);
        cx.notify();
    }

    /// Zoom in or out to the next step.
    pub(super) fn zoom_step(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        self.zoom_to(geometry::step_zoom(self.viewport.zoom, zoom_in), cx);
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

    /// Move the view so a block is in it, close enough to read, unless it
    /// already is. The view moves once the canvas has its size, which
    /// showing the inspector beside it may change.
    pub(super) fn reveal(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.viewport.zoom < geometry::DETAIL_ZOOM
            && let Some(layout) = self.layouts.get(id)
        {
            let center = point(
                layout.bounds.origin.x + layout.bounds.size.width / 2.,
                layout.bounds.origin.y + layout.bounds.size.height / 2.,
            );
            self.viewport.zoom = 1.;
            let visible = self.viewport.visible(self.view.size, self.rem);
            self.viewport.origin = point(
                center.x - visible.size.width / 2.,
                center.y - visible.size.height / 2.,
            );
        }
        self.pending_reveal = Some(id.to_owned());
        cx.notify();
    }

    /// Move the view as little as it takes to show a block. Returns whether
    /// it moved.
    pub(super) fn reveal_now(&mut self, id: &str) -> bool {
        let Some(layout) = self.layouts.get(id) else {
            return false;
        };
        match self
            .viewport
            .revealing(layout.bounds, self.view.size, self.rem)
        {
            Some(viewport) => {
                self.viewport = viewport;
                true
            }
            None => false,
        }
    }

    /// Where blocks added from the keyboard go: under the pointer when it
    /// is over the canvas, or the middle of the view.
    pub(super) fn add_position(&self) -> Point<f32> {
        match self.pointer {
            Some(pointer) if self.view.contains(&pointer) => self.canvas_position(pointer),
            _ => self.view_center(),
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

        let rem = window.rem_size();
        let inspector = self.render_inspector(cx);
        let inspected = inspector.is_some();
        let canvas = h_resizable("flow-inspector-split")
            .with_state(&self.inspector_split)
            .child(self.render_canvas(window, cx))
            .child(
                resizable_panel()
                    .visible(inspected)
                    .flex_none()
                    .size(rems(22.).to_pixels(rem))
                    .size_range(rems(16.).to_pixels(rem)..rems(44.).to_pixels(rem))
                    .children(inspector),
            );

        v_flex()
            .debug_selector(|| "flow-editor".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(self.render_toolbar(cx))
            .child(
                div().flex_1().min_h_0().min_w_0().overflow_hidden().child(
                    v_resizable("flow-log-split")
                        .with_state(&self.log_split)
                        .child(canvas)
                        .child(
                            resizable_panel()
                                .visible(self.log_open)
                                .flex_none()
                                .size(rems(13.).to_pixels(rem))
                                .size_range(rems(7.).to_pixels(rem)..rems(48.).to_pixels(rem))
                                .when(self.log_open, |this| this.child(self.render_run_log(cx))),
                        ),
                ),
            )
    }
}

impl Focusable for FlowEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
