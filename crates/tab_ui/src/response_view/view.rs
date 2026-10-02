use gpui_kit::base::{ElementExt as _, Tab, Tabs, TextSelectionScopeId};
use gpui_kit::component::{
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    kbd::Kbd,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{
    Execution, ExecutionError, HeaderMap, HttpMetrics, HttpResponse, Response, ServerSentEvent,
    StatusCode, Version,
};

use super::body::{Body, BodyMode};
use super::content::ResponseContent;
use super::events::EventLog;
use super::scripts::script_results;
use crate::actions::SendRequest;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Body,
    Cookies,
    Headers,
    Request,
    Tests,
    Console,
}

pub struct ResponseView {
    pub(super) focus: FocusHandle,
    pub(super) content: Option<ResponseContent>,
    /// Present exactly when `content` is.
    pub(super) body: Option<Body>,
    pub(super) mode: BodyMode,
    /// The events of an event-stream response, while it is open and after it ends.
    pub(super) events: Option<Entity<EventLog>>,
    pub(super) message: SharedString,
    loading: bool,
    error: bool,
    pub(super) scripts: Vec<request::ScriptReport>,
    section: Section,
    pub(super) wrap: bool,
    pub(super) headers_list: ListState,
    pub(super) cookies_list: ListState,
    pub(super) detail_open: [bool; 3],
    background_selection_scope: TextSelectionScopeId,
    /// Where the body was last saved, or why it could not be.
    pub(super) saved: Option<Result<std::path::PathBuf, SharedString>>,
    /// Counts the responses shown, so a save finishing late reports only on
    /// the response it saved.
    pub(super) responses: u64,
    pub(super) save_task: Option<Task<()>>,
    /// Whether a section shows the request as it went out.
    request_section: bool,
}

impl ResponseView {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: None,
            body: None,
            mode: BodyMode::Raw,
            events: None,
            message: "Send a request to see the response".into(),
            loading: false,
            error: false,
            scripts: Vec::new(),
            section: Section::Body,
            wrap: true,
            headers_list: ListState::new(0, ListAlignment::Top, px(0.)),
            cookies_list: ListState::new(0, ListAlignment::Top, px(0.)),
            detail_open: [false; 3],
            background_selection_scope: TextSelectionScopeId::new(),
            saved: None,
            responses: 0,
            save_task: None,
            request_section: false,
        }
    }

    /// Add a section that shows the request as it went out, as the
    /// Collection Runner does for each request it sends.
    pub(crate) fn with_request_section(mut self) -> Self {
        self.request_section = true;
        self
    }

    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        self.content = None;
        self.body = None;
        self.saved = None;
        self.responses += 1;
        self.events = None;
        self.scripts.clear();
        self.loading = true;
        self.error = false;
        self.message = "Sending request…".into();
        cx.notify();
    }

    /// An event-stream response opened. Its head shows now, and its events as
    /// they arrive.
    pub(crate) fn open_stream(
        &mut self,
        status: StatusCode,
        version: Version,
        headers: HeaderMap,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = ResponseContent::new(Execution {
            response: Response::Http(HttpResponse {
                status,
                version,
                headers,
                body: Vec::new(),
                metrics: HttpMetrics::default(),
            }),
            elapsed: std::time::Duration::ZERO,
            scripts: Vec::new(),
            sent: None,
        });

        self.headers_list.reset(content.headers.len());
        self.cookies_list.reset(content.cookies.len());
        let mode = content.default_mode();
        self.content = Some(content);
        self.show(mode, window, cx);
        self.events = Some(cx.new(|cx| EventLog::new(window, cx)));
        self.loading = false;
        cx.notify();
    }

    pub(crate) fn receive_events(&mut self, events: Vec<ServerSentEvent>, cx: &mut Context<Self>) {
        if let Some(log) = &self.events {
            log.update(cx, |log, cx| log.push(events, cx));
        }
    }

    pub(crate) fn stop_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(log) = &self.events {
            log.update(cx, |log, cx| log.stop(cx));
        }

        cx.notify();
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.loading = false;
        self.message = "Request cancelled".into();

        if let Some(log) = self.events.as_ref().filter(|log| log.read(cx).is_open()) {
            let message = self.message.clone();
            log.update(cx, |log, cx| log.finish(Some(message), cx));
        }

        cx.notify();
    }

    /// Show why a request from history failed after it was sent.
    pub(crate) fn fail(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.content = None;
        self.body = None;
        self.events = None;
        self.scripts.clear();
        self.loading = false;
        self.error = true;
        self.message = message;
        cx.notify();
    }

    pub fn finish(
        &mut self,
        result: Result<ResponseContent, ExecutionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading = false;
        self.scripts.clear();

        match result {
            Ok(mut content) => {
                self.scripts = std::mem::take(&mut content.execution.scripts);
                if self.scripts.iter().any(|report| {
                    report.error.is_some() || report.tests.iter().any(|test| test.error.is_some())
                }) {
                    self.section = Section::Tests;
                }
                self.headers_list.reset(content.headers.len());
                self.cookies_list.reset(content.cookies.len());
                let mode = content.default_mode();
                self.content = Some(content);
                self.show(mode, window, cx);
                self.saved = None;
                self.responses += 1;
                self.error = false;

                if let Some(log) = &self.events {
                    log.update(cx, |log, cx| log.finish(None, cx));
                }
            }
            Err(error) => {
                self.message = error.to_string().into();

                // Earlier scripts' reports wrap a failure or skip in a later script.
                let (source, reports) = match error {
                    ExecutionError::ScriptedRequest { source, reports } => (*source, Some(reports)),
                    error => (error, None),
                };
                self.error = !matches!(source, ExecutionError::Skipped { .. });
                match source {
                    ExecutionError::Skipped { report, .. } => {
                        self.scripts = vec![*report];
                        self.section = Section::Body;
                    }
                    ExecutionError::Script { report, .. } => {
                        self.scripts = vec![*report];
                        self.section = Section::Tests;
                    }
                    _ => {}
                }
                if let Some(reports) = reports {
                    self.scripts = reports;
                }

                if let Some(log) = &self.events {
                    // A broken stream keeps its head and events, ending with the error.
                    let message = self.message.clone();
                    log.update(cx, |log, cx| log.finish(Some(message), cx));
                    self.error = false;
                } else {
                    self.content = None;
                    self.body = None;
                }
            }
        }

        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let headers = self
            .content
            .as_ref()
            .map_or(0, |content| content.headers.len());
        let cookies = self
            .content
            .as_ref()
            .map_or(0, |content| content.cookies.len());

        h_flex()
            .flex_none()
            .min_w_0()
            .min_h_11()
            .gap_3()
            .flex_wrap()
            .child(
                Tabs::new("response-sections")
                    .flex()
                    .flex_row()
                    .flex_none()
                    .gap_1()
                    .children(
                        [
                            (
                                Section::Body,
                                if self.events.is_some() {
                                    "Events"
                                } else {
                                    "Body"
                                },
                                0,
                            ),
                            (Section::Cookies, "Cookies", cookies),
                            (Section::Headers, "Headers", headers),
                            (Section::Request, "Request", 0),
                            (
                                Section::Tests,
                                "Test Results",
                                self.scripts.iter().map(|report| report.tests.len()).sum(),
                            ),
                            (
                                Section::Console,
                                "Console",
                                self.scripts.iter().map(|report| report.logs.len()).sum(),
                            ),
                        ]
                        .into_iter()
                        .filter(|(section, _, _)| {
                            *section != Section::Request || self.request_section
                        })
                        .map(|(section, label, count)| {
                            let selected = self.section == section;

                            Tab::new(label)
                                .debug_selector(move || format!("response-section-{label}"))
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
            .when(self.content.is_some(), |row| {
                match self.events.as_ref().map(|events| events.read(cx)) {
                    Some(events) if events.is_open() => row.child(self.stream_status(true, cx)),
                    // A broken stream has its head, but no complete measurements.
                    Some(events) if events.failed() => row.child(self.stream_status(false, cx)),
                    _ => row.child(self.metadata(cx)),
                }
            })
    }

    fn empty_state(&self, window: &Window, cx: &App) -> AnyElement {
        let media = EmptyMedia::new().with_variant(EmptyMediaVariant::Icon);
        let header = if self.loading {
            EmptyHeader::new()
                .media(media.child(Icon::new(IconName::Loader).with_animation(
                    "response-loading",
                    Animation::new(std::time::Duration::from_secs(1)).repeat(),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                )))
                .title(EmptyTitle::new().child(self.message.clone()))
        } else if self.error {
            EmptyHeader::new()
                .media(
                    media.child(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger)),
                )
                .title(EmptyTitle::new().child("Request failed"))
                .description(EmptyDescription::new().child(self.message.clone()))
        } else {
            EmptyHeader::new()
                .media(media.child(Icon::default().path("icons/send-horizontal.svg")))
                .title(EmptyTitle::new().child(self.message.clone()))
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
                                    .child("to send"),
                            ),
                        )
                    },
                )
        };

        div()
            .debug_selector(|| "response-empty".into())
            .flex()
            .flex_1()
            .min_h_0()
            .child(Empty::new().header(header))
            .into_any_element()
    }
}

impl Render for ResponseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Starting a request clears the response and scripts, so a loading
        // view always falls through to the empty state.
        let has_results = self.content.is_some() || !self.scripts.is_empty();
        let has_response = self.content.is_some();

        let content = match self.section {
            Section::Tests if has_results => {
                script_results(&self.scripts, false, "send the request", cx)
            }
            Section::Console if has_results => {
                script_results(&self.scripts, true, "send the request", cx)
            }
            Section::Body if has_response => match &self.events {
                Some(events) => events.clone().into_any_element(),
                None => match self
                    .content
                    .as_ref()
                    .and_then(|content| content.omitted_body)
                {
                    Some(size) => omitted_body(size),
                    None => self.body(cx),
                },
            },
            Section::Headers if has_response => self.headers(false, cx),
            Section::Cookies if has_response => self.headers(true, cx),
            Section::Request if has_response => self.sent_request(cx),
            _ => self.empty_state(window, cx),
        };

        v_flex()
            .debug_selector(|| "response-panel".into())
            .key_context("Response")
            .track_focus(&self.focus)
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
            }))
            // Plain SelectableText updates its selection without invalidating
            // the owning view. Paint the changing highlight while dragging.
            .on_mouse_move(cx.listener(|_, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    cx.notify();
                }
            }))
            .size_full()
            .min_h_0()
            .min_w_0()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(has_results, |view| view.child(self.toolbar(cx)))
            .when(
                self.error
                    && !self.scripts.is_empty()
                    && self.scripts.iter().all(|report| report.error.is_none()),
                |view| {
                    view.child(
                        div()
                            .px_2()
                            .text_color(cx.theme().danger)
                            .child(self.message.clone()),
                    )
                },
            )
            .child(content)
            // Root owns the active scope. While a hover card is open, exclude
            // the response behind it from that scope; the card opts back in.
            .text_selection_scope(if self.detail_open.iter().any(|open| *open) {
                self.background_selection_scope
            } else {
                TextSelectionScopeId::default()
            })
    }
}

fn omitted_body(size: usize) -> AnyElement {
    div()
        .debug_selector(|| "response-body-omitted".into())
        .flex()
        .flex_1()
        .min_h_0()
        .child(
            Empty::new().header(
                EmptyHeader::new()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .child(Icon::new(IconName::Inbox)),
                    )
                    .title(EmptyTitle::new().child("Body not kept in history"))
                    .description(EmptyDescription::new().child(format!(
                        "History keeps bodies up to {}. This one is {}. Send the request again to see it.",
                        super::metadata::size_label(request_history::BODY_LIMIT),
                        super::metadata::size_label(size),
                    ))),
            ),
        )
        .into_any_element()
}
