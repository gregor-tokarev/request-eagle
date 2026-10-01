use std::collections::HashSet;
use std::time::{Duration, SystemTime};

use gpui_kit::base::{SelectableText, Tab, Tabs};
use gpui_kit::component::{
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Editor, EditorState, InputEvent, InputState},
    kbd::Kbd,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{GrpcError, GrpcEvent, GrpcStatus, MethodKind, ScriptReport};

use crate::actions::SendRequest;
use crate::response_view::script_results;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Response,
    Metadata,
    Trailers,
    Tests,
    Console,
}

pub(super) enum CallState {
    Idle,
    /// Preparing the call, such as loading the service definition, with a
    /// title saying what for.
    Waiting(SharedString),
    Running,
    Finished {
        status: GrpcStatus,
        elapsed: Duration,
    },
    /// The call could not start, or ended without a status.
    Failed(SharedString),
    /// Its Before invoke script skipped the call, for this reason.
    Skipped(SharedString),
    Cancelled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryKind {
    Sent,
    Received,
    Info,
    Completed,
    Error,
}

/// One row of the message stream.
pub(super) struct Entry {
    pub(super) kind: EntryKind,
    pub(super) at: SystemTime,
    /// A single line: compact JSON or an event description.
    pub(super) summary: SharedString,
    /// Indented JSON or error details, shown when the row is expanded.
    pub(super) detail: Option<SharedString>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Sent,
    Received,
}

/// The result of a gRPC call: the response message for unary methods and a
/// message stream for streaming methods, with metadata and trailers.
pub(crate) struct GrpcResponse {
    pub(super) state: CallState,
    pub(super) kind: Option<MethodKind>,
    pub(super) server: SharedString,
    pub(super) section: Section,
    pub(super) metadata: Vec<(SharedString, SharedString)>,
    pub(super) trailers: Vec<(SharedString, SharedString)>,
    /// In the order the scripts ran.
    pub(crate) scripts: Vec<ScriptReport>,
    /// The unary response message.
    pub(super) body: Option<Entity<EditorState>>,
    /// In arrival order. The stream shows the newest first.
    pub(super) entries: Vec<Entry>,
    /// Entries before this index were cleared from view and can be restored.
    pub(super) hidden: usize,
    /// Indices of the entries the stream shows, oldest first. The list shows
    /// them newest first.
    pub(super) shown: Vec<usize>,
    pub(super) expanded: HashSet<usize>,
    pub(super) filter: Filter,
    pub(super) search: Option<Entity<InputState>>,
    /// The lowercase search text.
    pub(super) query: String,
    pub(super) list: ListState,
    _subscriptions: Vec<Subscription>,
}

impl GrpcResponse {
    pub(crate) fn new(_: &mut Context<Self>) -> Self {
        Self {
            state: CallState::Idle,
            kind: None,
            server: SharedString::default(),
            section: Section::Response,
            metadata: Vec::new(),
            trailers: Vec::new(),
            scripts: Vec::new(),
            body: None,
            entries: Vec::new(),
            hidden: 0,
            shown: Vec::new(),
            expanded: HashSet::new(),
            filter: Filter::All,
            search: None,
            query: String::new(),
            list: ListState::new(0, ListAlignment::Top, px(0.)),
            _subscriptions: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.metadata.clear();
        self.trailers.clear();
        self.scripts.clear();
        self.body = None;
        self.entries.clear();
        self.hidden = 0;
        self.shown.clear();
        self.expanded.clear();
        self.section = Section::Response;
        self.list.reset(0);
    }

    /// Preparing the call before it starts, as `title` says.
    pub(crate) fn wait(&mut self, title: SharedString, cx: &mut Context<Self>) {
        self.reset();
        self.state = CallState::Waiting(title);
        cx.notify();
    }

    pub(crate) fn start(
        &mut self,
        kind: MethodKind,
        server: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset();
        self.kind = Some(kind);
        self.server = server;
        self.state = CallState::Running;
        self.push(Entry {
            kind: EntryKind::Info,
            at: SystemTime::now(),
            summary: format!("Sent request to {}", self.server).into(),
            detail: None,
        });

        if self.search.is_none() {
            let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
            self._subscriptions.push(cx.subscribe(
                &search,
                |this, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.query = search.read(cx).value().to_lowercase();
                        this.refresh_list();
                        cx.notify();
                    }
                },
            ));
            self.search = Some(search);
        }

        cx.notify();
    }

    /// A call that could not start, such as an invalid message.
    pub(crate) fn fail(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.reset();
        self.state = CallState::Failed(message);
        cx.notify();
    }

    /// A call that did not start. Its Before invoke script may have failed or
    /// skipped it, and its results show.
    pub(crate) fn fail_invoke(&mut self, error: GrpcError, cx: &mut Context<Self>) {
        self.reset();

        match error {
            GrpcError::Skipped { reason, report } => {
                self.state = CallState::Skipped(reason.into());
                self.scripts.push(*report);
            }
            GrpcError::Script { message, report } => {
                self.state = CallState::Failed(message.into());
                self.scripts.push(*report);
            }
            GrpcError::ScriptedCall { source, report } => {
                self.state = CallState::Failed(source.to_string().into());
                self.scripts.push(*report);
            }
            error => self.state = CallState::Failed(error.to_string().into()),
        }

        self.show_failures();
        cx.notify();
    }

    /// Like an HTTP response, show the tests when a script or test failed.
    fn show_failures(&mut self) {
        if self.scripts.iter().any(|report| {
            report.error.is_some() || report.tests.iter().any(|test| test.error.is_some())
        }) {
            self.section = Section::Tests;
        }
    }

    pub(crate) fn cancel(&mut self, call_started: bool, cx: &mut Context<Self>) {
        if call_started {
            self.push(Entry {
                kind: EntryKind::Error,
                at: SystemTime::now(),
                summary: "Operation cancelled".into(),
                detail: None,
            });
            self.state = CallState::Cancelled;
        } else {
            self.reset();
            self.state = CallState::Idle;
        }

        cx.notify();
    }

    pub(crate) fn receive(
        &mut self,
        events: Vec<GrpcEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for event in events {
            match event {
                GrpcEvent::Metadata(metadata) => {
                    self.metadata = pairs(metadata);
                    self.push(Entry {
                        kind: EntryKind::Info,
                        at: SystemTime::now(),
                        summary: format!("Received response from {}", self.server).into(),
                        detail: None,
                    });
                }
                GrpcEvent::Sent(message) => {
                    // Unary calls show only their response.
                    if self.kind != Some(MethodKind::Unary) {
                        self.push(message_entry(EntryKind::Sent, message));
                    }
                }
                GrpcEvent::Received(message) => {
                    if self.kind == Some(MethodKind::Unary) {
                        let text = message.json.clone();
                        self.body = Some(cx.new(|cx| {
                            EditorState::new(window, cx)
                                .language("json")
                                .line_number(true)
                                .soft_wrap(true)
                                .searchable(true)
                                .replaceable(false)
                                .default_value(text)
                        }));
                    }

                    self.push(message_entry(EntryKind::Received, message));
                }
                GrpcEvent::Finished {
                    status,
                    trailers,
                    elapsed,
                } => {
                    self.trailers = pairs(trailers);
                    self.push(Entry {
                        kind: if status.is_ok() {
                            EntryKind::Completed
                        } else {
                            EntryKind::Error
                        },
                        at: SystemTime::now(),
                        summary: if status.is_ok() {
                            "Call completed".into()
                        } else {
                            status.to_string().into()
                        },
                        detail: None,
                    });
                    self.state = CallState::Finished { status, elapsed };
                    self.show_failures();
                }
                GrpcEvent::Failed(error) => {
                    let message: SharedString = error.to_string().into();
                    self.push(Entry {
                        kind: EntryKind::Error,
                        at: SystemTime::now(),
                        summary: message.clone(),
                        detail: None,
                    });
                    self.state = CallState::Failed(message);
                    self.show_failures();
                }
                GrpcEvent::Script(report) => self.scripts.push(report),
            }
        }

        cx.notify();
    }

    fn push(&mut self, entry: Entry) {
        self.entries.push(entry);

        if self.shows(self.entries.len() - 1) {
            self.shown.push(self.entries.len() - 1);

            // The newest row is first. Keep it in view unless the stream
            // was scrolled down to read earlier messages.
            let top = self.list.logical_scroll_top();
            let at_top = top.item_ix == 0 && top.offset_in_item <= px(0.);
            self.list.splice(0..0, 1);

            if at_top {
                self.list.scroll_to(ListOffset::default());
            }
        }
    }

    /// Whether the stream shows the entry with the current filter and search.
    pub(super) fn shows(&self, index: usize) -> bool {
        let entry = &self.entries[index];

        index >= self.hidden
            && match self.filter {
                Filter::All => true,
                Filter::Sent => entry.kind == EntryKind::Sent,
                Filter::Received => entry.kind == EntryKind::Received,
            }
            && (self.query.is_empty() || entry.summary.to_lowercase().contains(&self.query))
    }

    /// The entry at a position of the list, which shows the newest first.
    pub(super) fn entry_at(&self, position: usize) -> Option<usize> {
        self.shown
            .len()
            .checked_sub(position + 1)
            .map(|index| self.shown[index])
    }

    /// Apply a changed filter, search or cleared history.
    pub(super) fn refresh_list(&mut self) {
        self.shown = (0..self.entries.len())
            .filter(|index| self.shows(*index))
            .collect();
        self.list.reset(self.shown.len());
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let single = self.kind == Some(MethodKind::Unary);
        let response_label = if single { "Response" } else { "Responses" };

        h_flex()
            .flex_none()
            .min_w_0()
            .min_h_11()
            .gap_3()
            .flex_wrap()
            .child(
                Tabs::new("grpc-response-sections")
                    .flex()
                    .flex_row()
                    .flex_none()
                    .gap_1()
                    .children(
                        [
                            (Section::Response, response_label, 0),
                            (Section::Metadata, "Metadata", self.metadata.len()),
                            (Section::Trailers, "Trailers", self.trailers.len()),
                            (
                                Section::Tests,
                                "Tests",
                                self.scripts.iter().map(|report| report.tests.len()).sum(),
                            ),
                            (
                                Section::Console,
                                "Console",
                                self.scripts.iter().map(|report| report.logs.len()).sum(),
                            ),
                        ]
                        .into_iter()
                        .map(|(section, label, count)| {
                            let selected = self.section == section;

                            Tab::new(label)
                                .debug_selector(move || format!("grpc-response-section-{label}"))
                                .selected(selected)
                                .h_8()
                                .px_2()
                                .gap_1()
                                .rounded(cx.theme().radius_tokens().md)
                                .text_color(cx.theme().muted_foreground)
                                .when(selected, |tab| {
                                    tab.bg(cx.theme().muted).text_color(cx.theme().foreground)
                                })
                                .hover(|tab| tab.bg(cx.theme().muted))
                                .child(label)
                                .when(count > 0, |tab| {
                                    tab.child(crate::section_count::section_count(
                                        count, selected, cx,
                                    ))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.section = section;
                                    cx.notify();
                                }))
                        }),
                    ),
            )
            .child(div().flex_1())
            .child(self.status(cx))
    }

    /// `Status code: 0 OK • 12 ms`, or a Streaming badge while a stream is open.
    fn status(&self, cx: &App) -> AnyElement {
        match &self.state {
            CallState::Running if self.kind != Some(MethodKind::Unary) => div()
                .debug_selector(|| "grpc-streaming".into())
                .px_2()
                .py_1()
                .rounded(cx.theme().radius_tokens().md)
                .bg(cx.theme().info.opacity(0.15))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().info)
                .child("STREAMING")
                .into_any_element(),
            CallState::Finished { status, elapsed } => status_code(
                format!("{} {}", status.code, status.name()),
                status.is_ok(),
                Some(*elapsed),
                cx,
            ),
            CallState::Cancelled => status_code("1 CANCELLED".into(), false, None, cx),
            _ => div().into_any_element(),
        }
    }

    fn empty_state(&self, window: &Window, cx: &App) -> AnyElement {
        let media = EmptyMedia::new().with_variant(EmptyMediaVariant::Icon);
        let header = match &self.state {
            CallState::Waiting(_) | CallState::Running => {
                let title = match &self.state {
                    CallState::Waiting(title) => title.clone(),
                    _ => "Waiting for the response…".into(),
                };

                EmptyHeader::new()
                    .media(media.child(Icon::new(IconName::Loader).with_animation(
                        "grpc-response-loading",
                        Animation::new(Duration::from_secs(1)).repeat(),
                        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                    )))
                    .title(EmptyTitle::new().child(title))
            }
            CallState::Failed(message) => EmptyHeader::new()
                .media(
                    media.child(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger)),
                )
                .title(EmptyTitle::new().child("Call failed"))
                .description(
                    EmptyDescription::new().child(
                        div()
                            .debug_selector(|| "grpc-error".into())
                            .child(message.clone()),
                    ),
                ),
            CallState::Finished { status, .. } if !status.is_ok() => EmptyHeader::new()
                .media(
                    media.child(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger)),
                )
                .title(EmptyTitle::new().child(format!("{} {}", status.code, status.name())))
                .description(
                    EmptyDescription::new().child(
                        div()
                            .debug_selector(|| "grpc-error".into())
                            .child(status_description(status)),
                    ),
                ),
            CallState::Skipped(reason) => EmptyHeader::new()
                .media(media.child(Icon::new(IconName::CircleX)))
                .title(EmptyTitle::new().child("Call skipped"))
                .description(
                    EmptyDescription::new().child(
                        div()
                            .debug_selector(|| "grpc-skipped".into())
                            .child(reason.clone()),
                    ),
                ),
            CallState::Cancelled => EmptyHeader::new()
                .media(media.child(Icon::new(IconName::CircleX).text_color(cx.theme().danger)))
                .title(EmptyTitle::new().child("Operation cancelled")),
            CallState::Finished { .. } => EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Inbox)))
                .title(EmptyTitle::new().child("The server sent no message")),
            CallState::Idle => EmptyHeader::new()
                .media(media.child(Icon::default().path("icons/send-horizontal.svg")))
                .title(EmptyTitle::new().child("Invoke a method to see the response"))
                .when_some(
                    Kbd::binding_for_action(&SendRequest, Some("Workspace"), window),
                    |header, kbd| {
                        header.description(
                            EmptyDescription::new().child(
                                h_flex()
                                    .justify_center()
                                    .gap_1()
                                    .child("Press")
                                    .child(kbd)
                                    .child("to invoke"),
                            ),
                        )
                    },
                ),
        };

        div()
            .debug_selector(|| "grpc-response-empty".into())
            .flex()
            .flex_1()
            .min_h_0()
            .child(Empty::new().header(header))
            .into_any_element()
    }

    fn pairs_table(
        &self,
        id: &'static str,
        rows: &[(SharedString, SharedString)],
        empty: &'static str,
        cx: &App,
    ) -> AnyElement {
        if rows.is_empty() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(empty)
                .into_any_element();
        }

        v_flex()
            .id(id)
            .debug_selector(move || id.into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                h_flex()
                    .h_8()
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(rems(15.)).flex_shrink_0().px_2().child("Key"))
                    .child(div().flex_1().px_2().child("Value")),
            )
            .children(rows.iter().enumerate().map(|(index, (name, value))| {
                h_flex()
                    .w_full()
                    .items_start()
                    .min_h_8()
                    .py_2()
                    .when(index > 0, |row| row.border_t_1())
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .w(rems(15.))
                            .flex_shrink_0()
                            .px_2()
                            .font_family(cx.theme().mono_font_family.clone())
                            .cursor_text()
                            .child(
                                SelectableText::new((id, index * 2), name.clone())
                                    .document_order((index * 2) as u64),
                            ),
                    )
                    .child(
                        div().flex_1().min_w_0().px_2().cursor_text().child(
                            SelectableText::new((id, index * 2 + 1), value.clone())
                                .document_order((index * 2 + 1) as u64),
                        ),
                    )
            }))
            .into_any_element()
    }
}

impl Render for GrpcResponse {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = !matches!(
            self.state,
            CallState::Idle | CallState::Waiting(_) | CallState::Failed(_) | CallState::Skipped(_)
        ) || !self.entries.is_empty();
        let unary = self.kind == Some(MethodKind::Unary);
        let has_results = started || !self.scripts.is_empty();

        let content = match self.section {
            Section::Tests | Section::Console if has_results => script_results(
                &self.scripts,
                self.section == Section::Console,
                "invoke the method",
                cx,
            ),
            _ if !started => self.empty_state(window, cx),
            Section::Metadata => {
                self.pairs_table("grpc-response-metadata", &self.metadata, "No metadata", cx)
            }
            Section::Trailers => {
                self.pairs_table("grpc-response-trailers", &self.trailers, "No trailers", cx)
            }
            Section::Response if unary => match &self.body {
                Some(body) => div()
                    .debug_selector(|| "grpc-response-body".into())
                    .flex_1()
                    .min_h_0()
                    .child(
                        Editor::new(body)
                            .h_full()
                            .readonly(true)
                            .appearance(false)
                            .bordered(false)
                            .bg(cx
                                .theme()
                                .highlight_theme
                                .style
                                .editor_background
                                .unwrap_or_else(|| cx.theme().input_background()))
                            .text_sm()
                            .aria_label("Response message"),
                    )
                    .into_any_element(),
                None => self.empty_state(window, cx),
            },
            _ => self.stream(cx),
        };

        v_flex()
            .debug_selector(|| "grpc-response".into())
            .key_context("Response")
            .size_full()
            .min_h_0()
            .min_w_0()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            // Plain SelectableText paints its selection while dragging.
            .on_mouse_move(cx.listener(|_, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    cx.notify();
                }
            }))
            .when(has_results, |view| view.child(self.toolbar(cx)))
            .child(content)
    }
}

/// `Status code: 0 OK • 12 ms`, as Postman shows it.
fn status_code(text: String, ok: bool, elapsed: Option<Duration>, cx: &App) -> AnyElement {
    let color = if ok {
        cx.theme().success
    } else {
        cx.theme().danger
    };

    h_flex()
        .debug_selector(|| "grpc-status".into())
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("Status code:")
        .child(
            div()
                .debug_selector(|| "grpc-status-code".into())
                .px_2()
                .py_1()
                .rounded(cx.theme().radius_tokens().md)
                .bg(color.opacity(0.15))
                .text_color(color)
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
        )
        .when_some(elapsed, |row, elapsed| {
            row.child("•").child(
                div()
                    .debug_selector(|| "grpc-elapsed".into())
                    .child(duration_label(elapsed)),
            )
        })
        .into_any_element()
}

fn message_entry(kind: EntryKind, message: request::GrpcMessage) -> Entry {
    // The summary is the message on one line, like Postman's stream.
    let summary = serde_json::from_str::<serde_json::Value>(&message.json)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| message.json.clone());

    Entry {
        kind,
        at: message.at,
        summary: summary.into(),
        detail: Some(message.json.into()),
    }
}

fn pairs(pairs: Vec<(String, String)>) -> Vec<(SharedString, SharedString)> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect()
}

fn duration_label(duration: Duration) -> String {
    format!("{} ms", duration.as_millis())
}

/// The server's message, or what the status usually means.
fn status_description(status: &GrpcStatus) -> String {
    if !status.message.is_empty() {
        return status.message.clone();
    }

    match status.name() {
        "UNAVAILABLE" => {
            "The service is unavailable. Check the URL, and whether the server requires TLS."
        }
        "UNIMPLEMENTED" => "The server does not implement this method.",
        "UNAUTHENTICATED" => "The server requires credentials. Add them in Metadata.",
        "PERMISSION_DENIED" => "The credentials do not allow this call.",
        "DEADLINE_EXCEEDED" => "The call did not finish in time.",
        "INVALID_ARGUMENT" => "The server rejected the message.",
        _ => "The server ended the call with an error.",
    }
    .to_owned()
}
