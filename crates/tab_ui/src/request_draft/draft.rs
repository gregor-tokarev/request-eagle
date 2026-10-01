use std::path::{Path, PathBuf};

use super::fields::{FieldsChanged, RequestFields};
use crate::response_view::{ResponseContent, ResponseView};
use crate::{
    Environments, RequestSent,
    script_editor::{ScriptEditor, ScriptTarget, ScriptsChanged},
    variable_input::{VariableInput, VariableTarget},
    variables::VariableScope,
};
use environment::EnvironmentSessions;
use gpui_kit::component::resizable::{ResizableState, resizable_panel, v_resizable};
use gpui_kit::component::{
    input::{EditorState, InputEvent, InputState},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::*;
use request::{HttpRequest, Method};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestSection {
    Params,
    Headers,
    Body,
    Scripts,
}

/// Where a saved request is stored, and how the collections sidebar names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestLocation {
    pub path: PathBuf,
    pub id: SharedString,
    pub name: SharedString,
    pub collection: SharedString,
    pub folders: Vec<SharedString>,
}

impl RequestLocation {
    /// The directory of the collection that stores the request.
    pub(crate) fn collection_path(&self) -> Option<PathBuf> {
        self.path
            .ancestors()
            .nth(self.folders.len() + 1)
            .map(Path::to_path_buf)
    }

    /// The collection environment that the request's variables resolve from.
    pub(crate) fn environment_path(&self) -> Option<PathBuf> {
        self.collection_path()
            .map(|collection| collection.join("environment.toml"))
    }
}

/// An editable HTTP request snapshot owned by one tab, independent of collection storage.
pub struct RequestDraft {
    /// Unsaved drafts have no location.
    pub location: Option<RequestLocation>,
    pub request: HttpRequest,
    saved_request: HttpRequest,
    pub(crate) url: Option<Entity<InputState>>,
    pub(crate) section: RequestSection,
    pub(super) params: Option<Entity<RequestFields>>,
    pub(super) headers: Option<Entity<RequestFields>>,
    pub(super) generated_headers: Vec<(String, String)>,
    pub(super) body: Option<Entity<EditorState>>,
    pub(super) body_vim: Option<Entity<crate::vim::Vim>>,
    pub(super) body_json_valid: bool,
    pub(super) body_task: Option<Task<()>>,
    pub(crate) scripts: Option<Entity<ScriptEditor>>,
    pub(super) variables: Entity<VariableScope>,
    pub(super) variable_sessions: EnvironmentSessions,
    pub(super) url_completion: Option<Entity<VariableInput>>,
    pub(super) body_completion: Option<Entity<VariableInput>>,
    pub(super) response: Entity<ResponseView>,
    split: Entity<ResizableState>,
    pub(super) task: Option<Task<()>>,
    /// The request being sent, which history keeps if it is cancelled after
    /// it went out.
    pub(super) sending: Option<(RequestSent, request::Dispatch)>,
    /// Ends the response if it is an event stream. Taken when it is stopped.
    pub(super) stop: Option<request::StopEventStream>,
    /// Whether the response is an event stream that has not ended yet.
    pub(super) streaming: bool,
    pub(super) executor: Option<(request::RequestPreferences, request::RequestExecutor)>,
    address: Entity<RequestAddress>,
    configuration: Entity<RequestConfiguration>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestSent> for RequestDraft {}

impl RequestDraft {
    /// Variables resolve from the request's collection environment, its
    /// session values and the active global environment.
    pub fn new(
        request: HttpRequest,
        location: Option<RequestLocation>,
        sessions: EnvironmentSessions,
        environments: Option<Entity<Environments>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let environment_path = location
            .as_ref()
            .and_then(RequestLocation::environment_path);
        // Switching the active environment changes which references resolve.
        let subscriptions = environments
            .iter()
            .map(|environments| {
                cx.observe(environments, |this: &mut Self, _, cx| {
                    this.variables.update(cx, |scope, cx| scope.changed(cx));
                })
            })
            .collect();
        let variables = cx.new(|_| VariableScope {
            session: sessions.for_path(environment_path.as_deref()),
            path: environment_path,
            environments,
            names: None,
        });

        // Unlike the editors, these views do not install window listeners or
        // notify while they are created, so unvisited tabs can own them.
        let owner = cx.weak_entity();
        let response = cx.new(|cx| ResponseView::new(cx));
        let split = cx.new(|_| ResizableState::default());
        let address = cx.new(|_| RequestAddress(owner.clone()));
        let configuration = cx.new(|_| RequestConfiguration(owner));

        Self {
            location,
            generated_headers: super::execution::generated_headers(&request),
            saved_request: request.clone(),
            request,
            url: None,
            section: RequestSection::Headers,
            params: None,
            headers: None,
            body: None,
            body_vim: None,
            body_json_valid: false,
            body_task: None,
            scripts: None,
            variables,
            variable_sessions: sessions,
            url_completion: None,
            body_completion: None,
            response,
            split,
            task: None,
            sending: None,
            stop: None,
            streaming: false,
            executor: None,
            address,
            configuration,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.request != self.saved_request
    }

    /// Redraw the cached URL bar for a change that did not come from its input.
    pub(super) fn notify_address(&self, cx: &mut Context<Self>) {
        self.address.update(cx, |_, cx| cx.notify());
    }

    /// Follow the saved request to its current file and name.
    pub fn set_location(&mut self, location: RequestLocation, cx: &mut Context<Self>) {
        let path = location.environment_path();
        let session = self.variable_sessions.for_path(path.as_deref());

        self.variables.update(cx, |scope, cx| {
            scope.path = path;
            scope.session = session;
            scope.changed(cx);
        });

        self.location = Some(location);
        cx.notify();
    }

    /// Show the response, or the failure, that history kept for this request.
    pub fn show_recorded(
        &mut self,
        response: Option<request_history::Response>,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.response
            .update(cx, |view, cx| match (response, error) {
                (Some(response), _) => {
                    let content = ResponseContent::recorded(response);
                    view.finish(Ok(content), window, cx);
                }
                (None, Some(error)) => view.fail(error.into(), cx),
                (None, None) => {}
            });
    }

    pub fn mark_saved(&mut self, request: HttpRequest, cx: &mut Context<Self>) {
        self.saved_request = request;
        cx.notify();
    }

    pub fn set_method(&mut self, method: Method, cx: &mut Context<Self>) {
        self.request.method = method;
        self.refresh_generated_headers(cx);

        if !self.supports_body() && self.section == RequestSection::Body {
            self.section = RequestSection::Headers;
        }

        cx.notify();
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.variables.update(cx, |scope, cx| scope.changed(cx));

        // Initialize newly activated controls before drawing. Their setup can
        // notify GPUI; doing it inside render schedules an unnecessary frame.
        self.url_state(window, cx);

        if self.section == RequestSection::Body {
            self.body_state(window, cx);
        } else if self.section == RequestSection::Scripts {
            self.script_state(window, cx);
        } else {
            self.fields_state(window, cx);
        }

        self.refresh_generated_headers(cx);
    }

    pub(super) fn url_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let scope = self.variables.clone();
        // Unvisited tabs only need request data. Creating an InputState also
        // registers window and keystroke listeners, so wait until it is visible.
        self.url
            .get_or_insert_with(|| {
                let url = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Enter URL or paste text")
                        .default_value(self.request.path.clone())
                });
                self.url_completion = Some(cx.new(|cx| {
                    VariableInput::new(VariableTarget::Input(url.clone()), scope, window, cx)
                }));
                self._subscriptions.push(cx.subscribe(
                    &url,
                    |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.request.path = input.read(cx).value().to_string();
                            this.refresh_generated_headers(cx);
                            cx.notify();
                        }
                    },
                ));

                url
            })
            .clone()
    }

    pub(super) fn script_editor(&mut self, cx: &mut Context<Self>) -> Entity<ScriptEditor> {
        self.scripts
            .get_or_insert_with(|| {
                let scripts = cx.new(|_| {
                    ScriptEditor::new(self.request.scripts.clone(), ScriptTarget::Request)
                });
                self._subscriptions.push(cx.subscribe(
                    &scripts,
                    |this, _, event: &ScriptsChanged, cx| {
                        this.request.scripts = event.0.clone();
                        cx.notify();
                    },
                ));

                scripts
            })
            .clone()
    }

    /// The editor of the selected script phase.
    pub(super) fn script_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        self.script_editor(cx)
            .update(cx, |scripts, cx| scripts.editor(window, cx))
    }

    fn fields_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        let scope = self.variables.clone();
        let is_headers = self.section == RequestSection::Headers;
        let slot = if is_headers {
            &mut self.headers
        } else {
            &mut self.params
        };

        if slot.is_none() {
            let id = if is_headers { "headers" } else { "params" };
            let values = if is_headers {
                self.request.headers.as_slice()
            } else {
                self.request.query.as_slice()
            };
            let generated = if is_headers {
                self.generated_headers.as_slice()
            } else {
                &[]
            };
            let fields = cx.new(|cx| RequestFields::new(id, values, generated, scope, window, cx));
            let subscription = cx.subscribe(&fields, move |this, _, event: &FieldsChanged, cx| {
                if is_headers {
                    this.request.headers = event.0.clone();
                    this.refresh_generated_headers(cx);
                } else {
                    this.request.query = event.0.clone();
                }

                cx.notify();
            });
            self._subscriptions.push(subscription);
            *slot = Some(fields);
        }

        slot.as_ref().unwrap().clone()
    }

    fn fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // The selected section tab already names the table.
        self.fields_state(window, cx).into_any_element()
    }
}

impl Render for RequestDraft {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "request-draft".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_sm()
            .child(
                self.address
                    .clone()
                    .cached(StyleRefinement::default().w_full().h_20().flex_none()),
            )
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    v_resizable("request-response-split")
                        .with_state(&self.split)
                        .child(
                            resizable_panel()
                                .size(rems(20.).to_pixels(window.rem_size()))
                                .size_range(
                                    rems(14.).to_pixels(window.rem_size())
                                        ..rems(75.).to_pixels(window.rem_size()),
                                )
                                .child(
                                    self.configuration
                                        .clone()
                                        .cached(StyleRefinement::default().size_full()),
                                ),
                        )
                        .child(
                            resizable_panel()
                                .size_range(rems(12.).to_pixels(window.rem_size())..Pixels::MAX)
                                .child(self.response.clone()),
                        ),
                ),
            )
    }
}

// Cache the editable controls independently of the response. Notifications
// from their draft or input states invalidate these views, while selecting a
// response does not redraw the address bar and request fields.
struct RequestAddress(WeakEntity<RequestDraft>);

impl Render for RequestAddress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(super::controls::request_header(
                        "HTTP",
                        draft.location.as_ref(),
                        cx,
                    ))
                    .child(draft.url_bar(window, cx))
            })
            .unwrap_or_else(|_| div())
    }
}

struct RequestConfiguration(WeakEntity<RequestDraft>);

impl Render for RequestConfiguration {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                let content = match draft.section {
                    RequestSection::Headers | RequestSection::Params => draft.fields(window, cx),
                    RequestSection::Body => draft.body(window, cx),
                    RequestSection::Scripts => draft.script_editor(cx).into_any_element(),
                };

                v_flex()
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .gap_2()
                    .pb_3()
                    .child(draft.section_tabs(cx))
                    .child(
                        div()
                            .id("request-section-content")
                            .debug_selector(|| "request-section-content".into())
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(content),
                    )
            })
            .unwrap_or_else(|_| div())
    }
}
