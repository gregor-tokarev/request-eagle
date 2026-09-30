use gpui_kit::base::{ElementExt as _, Tab, Tabs, TextSelectionScopeId};
use gpui_kit::component::{
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    kbd::Kbd,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ExecutionError;

use super::body::Body;
use super::content::ResponseContent;
use crate::actions::SendRequest;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Body,
    Cookies,
    Headers,
    Tests,
    Console,
}

pub struct ResponseView {
    pub(super) focus: FocusHandle,
    pub(super) content: Option<ResponseContent>,
    /// Present exactly when `content` is.
    pub(super) body: Option<Body>,
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
}

impl ResponseView {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: None,
            body: None,
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
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        self.content = None;
        self.body = None;
        self.scripts.clear();
        self.loading = true;
        self.error = false;
        self.message = "Sending request…".into();
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.loading = false;
        self.message = "Request cancelled".into();
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
                self.content = Some(content);
                self.set_pretty(true, window, cx);
                self.error = false;
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
                self.content = None;
                self.body = None;
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
                            (Section::Body, "Body", 0),
                            (Section::Cookies, "Cookies", cookies),
                            (Section::Headers, "Headers", headers),
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
            .when(self.content.is_some(), |row| row.child(self.metadata(cx)))
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
            Section::Tests if has_results => self.script_results(false, cx),
            Section::Console if has_results => self.script_results(true, cx),
            Section::Body if has_response => self.body(cx),
            Section::Headers if has_response => self.headers(false, cx),
            Section::Cookies if has_response => self.headers(true, cx),
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
