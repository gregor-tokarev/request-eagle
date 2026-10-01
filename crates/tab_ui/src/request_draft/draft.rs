use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::fields::{FieldsChanged, RequestFields};
use super::path_variables::{PathVariableChanged, PathVariables};
use crate::code_snippet::{self, SnippetDraft, SnippetPanel};
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
    notification::Notification,
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{HttpRequest, Method};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestSection {
    Params,
    Headers,
    Body,
    Scripts,
    Settings,
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
    /// The URL's query parameters.
    pub(super) params: Option<Entity<RequestFields>>,
    pub(super) path_variables: Option<Entity<PathVariables>>,
    /// The values given to path variables in this tab. A value stays while
    /// its variable is out of the URL, so it returns with the variable.
    path_values: Vec<(String, String)>,
    pub(super) headers: Option<Entity<RequestFields>>,
    pub(super) generated_headers: Vec<(String, String)>,
    pub(super) body: Option<Entity<EditorState>>,
    pub(super) body_vim: Option<Entity<crate::vim::Vim>>,
    pub(super) body_json_valid: bool,
    pub(super) body_task: Option<Task<()>>,
    pub(crate) scripts: Option<Entity<ScriptEditor>>,
    pub(super) timeout: Option<Entity<InputState>>,
    pub(super) variables: Entity<VariableScope>,
    pub(super) variable_sessions: EnvironmentSessions,
    pub(super) url_completion: Option<Entity<VariableInput>>,
    pub(super) body_completion: Option<Entity<VariableInput>>,
    pub(super) response: Entity<ResponseView>,
    split: Entity<ResizableState>,
    /// The request as a cURL command, beside it while open.
    pub(super) code_snippet: SnippetPanel<Self>,
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

impl SnippetDraft for RequestDraft {
    type Request = HttpRequest;
    const PROGRAM: &'static str = "cURL";

    fn request(&self) -> &HttpRequest {
        &self.request
    }

    fn command(&self, values: &HashMap<String, String>) -> String {
        self.request.curl_command(values)
    }

    fn variables(&self) -> &Entity<VariableScope> {
        &self.variables
    }

    fn snippet_panel(&mut self) -> &mut SnippetPanel<Self> {
        &mut self.code_snippet
    }

    fn toggle_code_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        code_snippet::toggle(self, window, cx);
        self.notify_address(cx);
        cx.notify();
    }
}

impl RequestDraft {
    /// Variables resolve from the request's collection environment, its
    /// session values and the active global environment.
    pub fn new(
        mut request: HttpRequest,
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
        request.inline_query();

        Self {
            location,
            generated_headers: super::execution::generated_headers(&request),
            path_values: request.path_variables.clone(),
            saved_request: request.clone(),
            request,
            url: None,
            section: RequestSection::Headers,
            params: None,
            path_variables: None,
            headers: None,
            body: None,
            body_vim: None,
            body_json_valid: false,
            body_task: None,
            scripts: None,
            timeout: None,
            variables,
            variable_sessions: sessions,
            url_completion: None,
            body_completion: None,
            response,
            split,
            code_snippet: SnippetPanel::default(),
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

    /// The query parameters and path variables in the URL.
    pub(super) fn params_count(&self) -> usize {
        request::query_params(&self.request.path).len()
            + path_variable_names(&self.request.path).len()
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

    pub fn mark_saved(&mut self, mut request: HttpRequest, cx: &mut Context<Self>) {
        request.inline_query();
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

    /// Replace the request with the one a pasted cURL command sends. Its
    /// scripts stay. A command that cannot be read is explained instead.
    pub(super) fn paste_curl(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = match import::parse_curl(command) {
            Ok(request) => request,
            Err(error) => {
                window.push_notification(
                    Notification::error(error.to_string()).title("Could not import cURL"),
                    cx,
                );
                return;
            }
        };

        self.request = HttpRequest {
            scripts: std::mem::take(&mut self.request.scripts),
            ..request
        };

        // The URL keeps focus. Path values given before belong to the replaced
        // request. The other editors are created again from the new request
        // when they are shown.
        self.path_values.clear();
        if let Some(url) = &self.url {
            let path = self.request.path.clone();
            url.update(cx, |url, cx| url.set_value(path, window, cx));
        }
        if let Some(url) = &self.url_completion {
            url.update(cx, |url, cx| url.set_path_variables(HashSet::new(), cx));
        }
        self.params = None;
        self.path_variables = None;
        self.headers = None;
        self.body = None;
        self.body_vim = None;
        self.body_completion = None;
        self.body_task = None;

        self.set_method(self.request.method, cx);
        self.prepare(window, cx);
        self.notify_address(cx);
    }

    /// Copies the request as a cURL command, with the variables that resolve
    /// filled in.
    pub fn copy_as_curl(&self, window: &mut Window, cx: &mut App) {
        code_snippet::copy(self, window, cx);
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.variables.update(cx, |scope, cx| scope.changed(cx));

        // Initialize newly activated controls before drawing. Their setup can
        // notify GPUI; doing it inside render schedules an unnecessary frame.
        self.url_state(window, cx);

        match self.section {
            RequestSection::Params => {
                self.params_state(window, cx);
            }
            RequestSection::Headers => {
                self.headers_state(window, cx);
            }
            RequestSection::Body => {
                self.body_state(window, cx);
            }
            RequestSection::Scripts => {
                self.script_state(window, cx);
            }
            RequestSection::Settings => {
                self.timeout_state(window, cx);
            }
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
                let filled = filled_path_variables(&self.path_values);
                self.url_completion = Some(cx.new(|cx| {
                    VariableInput::new(VariableTarget::Input(url.clone()), scope, window, cx)
                        .with_path_variables(filled)
                }));
                self._subscriptions.push(cx.subscribe_in(
                    &url,
                    window,
                    |this, input, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.request.path = input.read(cx).value().to_string();
                            this.url_changed(window, cx);
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

    /// Show an edit of the URL in the Params tables.
    fn url_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let names = path_variable_names(&self.request.path);
        self.keep_path_values_in_url();

        if let Some(params) = &self.params {
            let values = request::query_params(&self.request.path);
            params.update(cx, |params, cx| params.set_values(&values, window, cx));
        }
        if let Some(table) = &self.path_variables {
            table.update(cx, |table, cx| {
                table.set_names(&names, &self.path_values, window, cx)
            });
        }

        self.refresh_generated_headers(cx);
        cx.notify();
    }

    /// Write the Params table's rows into the URL's query.
    fn set_query(
        &mut self,
        params: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = request::with_query_params(&self.request.path, params);
        if path == self.request.path {
            return;
        }

        if let Some(url) = &self.url {
            url.update(cx, |url, cx| url.set_value(path.clone(), window, cx));
        }
        self.request.path = path;

        self.refresh_generated_headers(cx);
        self.notify_address(cx);
        cx.notify();
    }

    /// Send and save the values of the path variables in the URL.
    fn keep_path_values_in_url(&mut self) {
        let names = path_variable_names(&self.request.path);
        self.request.path_variables = self
            .path_values
            .iter()
            .filter(|(name, _)| names.contains(name))
            .cloned()
            .collect();
    }

    /// Keep a value typed in the Path Variables table. Its variable is in
    /// the URL, since the table shows only those.
    fn set_path_value(&mut self, changed: &PathVariableChanged, cx: &mut Context<Self>) {
        let PathVariableChanged { name, value } = changed;
        let known = self.path_values.iter().position(|(known, _)| known == name);

        match known {
            Some(index) if value.is_empty() => {
                self.path_values.remove(index);
            }
            Some(index) => self.path_values[index].1 = value.clone(),
            None if !value.is_empty() => self.path_values.push((name.clone(), value.clone())),
            None => {}
        }

        self.keep_path_values_in_url();

        let filled = filled_path_variables(&self.path_values);
        if let Some(url) = &self.url_completion {
            url.update(cx, |url, cx| url.set_path_variables(filled, cx));
        }
        cx.notify();
    }

    fn headers_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        if let Some(headers) = &self.headers {
            return headers.clone();
        }

        let scope = self.variables.clone();
        let headers = cx.new(|cx| {
            RequestFields::new(
                "headers",
                &self.request.headers,
                &self.generated_headers,
                scope,
                window,
                cx,
            )
        });
        self._subscriptions.push(
            cx.subscribe(&headers, |this, _, event: &FieldsChanged, cx| {
                this.request.headers = event.0.clone();
                this.refresh_generated_headers(cx);
                cx.notify();
            }),
        );
        self.headers = Some(headers.clone());

        headers
    }

    fn params_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<RequestFields>, Entity<PathVariables>) {
        if let (Some(params), Some(path_variables)) = (&self.params, &self.path_variables) {
            return (params.clone(), path_variables.clone());
        }

        let scope = self.variables.clone();
        let values = request::query_params(&self.request.path);
        let params = cx.new(|cx| {
            RequestFields::new("params", &values, &[], scope.clone(), window, cx)
                .with_keyless_rows()
        });
        self._subscriptions.push(cx.subscribe_in(
            &params,
            window,
            |this, _, event: &FieldsChanged, window, cx| this.set_query(&event.0, window, cx),
        ));

        let names = path_variable_names(&self.request.path);
        let path_variables =
            cx.new(|cx| PathVariables::new(&names, &self.path_values, scope, window, cx));
        self._subscriptions.push(cx.subscribe(
            &path_variables,
            |this, _, event: &PathVariableChanged, cx| this.set_path_value(event, cx),
        ));

        self.params = Some(params.clone());
        self.path_variables = Some(path_variables.clone());

        (params, path_variables)
    }

    /// The Query Params table, and the Path Variables table when the URL has
    /// `:name` segments.
    fn params(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (params, path_variables) = self.params_state(window, cx);
        let label = |text: &'static str| {
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(text)
        };

        v_flex()
            .gap_4()
            .child(v_flex().gap_2().child(label("Query Params")).child(params))
            .when(!path_variables.read(cx).is_empty(), |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(label("Path Variables"))
                        .child(path_variables),
                )
            })
            .into_any_element()
    }
}

/// The names of the URL's path variables, each once, in URL order.
fn path_variable_names(url: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();

    for (_, name) in request::path_variables(url) {
        if !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }

    names
}

/// The path variables that are filled in when the request is sent.
fn filled_path_variables(values: &[(String, String)]) -> HashSet<String> {
    values
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, _)| name.clone())
        .collect()
}

impl Render for RequestDraft {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let request = v_flex()
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
            );

        code_snippet::with_snippet(self, request.into_any_element(), cx)
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
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(super::controls::request_header(
                                        "HTTP",
                                        draft.location.as_ref(),
                                        cx,
                                    )),
                            )
                            .child(code_snippet::toggle_button(
                                draft.code_snippet.snippet.is_some(),
                                cx,
                            )),
                    )
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
                    // The selected section tab already names the table.
                    RequestSection::Headers => draft.headers_state(window, cx).into_any_element(),
                    RequestSection::Params => draft.params(window, cx),
                    RequestSection::Body => draft.body(window, cx),
                    RequestSection::Scripts => draft.script_editor(cx).into_any_element(),
                    RequestSection::Settings => draft.settings(window, cx),
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
