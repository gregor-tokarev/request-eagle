use std::collections::HashMap;
use std::path::PathBuf;

use collection::SavedLocation;
use environment::EnvironmentSessions;
use gpui_kit::component::resizable::{ResizableState, resizable_panel, v_resizable};
use gpui_kit::component::{
    input::{EditorState, InputEvent, InputState},
    scroll::ScrollableElement as _,
    select::{SelectEvent, SelectState},
    *,
};
use gpui_kit::*;
use request::{Auth, Field, GrpcClient, GrpcRequest, GrpcScripts, MethodKind, RequestPreferences};

use super::definition::DefinitionState;
use super::methods::{MethodList, method_list};
use crate::auth_editor::{AuthChanged, AuthEditor, AuthTarget, Inherited};
use crate::code_snippet::{self, SnippetDraft, SnippetPanel};
use crate::grpc_response::GrpcResponse;
use crate::request_draft::{FieldsChanged, RequestFields};
use crate::script_editor::{ScriptEditor, ScriptTarget, ScriptsChanged};
use crate::{
    Environments, RequestSent,
    variable_input::{VariableInput, VariableTarget},
    variables::VariableScope,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrpcSection {
    Message,
    Auth,
    Metadata,
    Definition,
    Scripts,
    Settings,
}

/// An editable gRPC request owned by one tab: its address, method, message,
/// metadata and service definition, and the call it is running.
pub struct GrpcDraft {
    /// Unsaved drafts have no location.
    pub location: Option<SavedLocation>,
    /// The name given to the request in its tab before it is saved.
    pub name: Option<SharedString>,
    pub request: GrpcRequest,
    saved_request: GrpcRequest,
    pub(crate) section: GrpcSection,
    pub(super) url: Option<Entity<InputState>>,
    pub(super) url_completion: Option<Entity<VariableInput>>,
    pub(super) message: Option<Entity<EditorState>>,
    pub(super) message_completion: Option<Entity<VariableInput>>,
    pub(super) message_vim: Option<Entity<crate::vim::Vim>>,
    pub(super) message_json_valid: bool,
    pub(super) metadata: Option<Entity<RequestFields>>,
    pub(super) auth: Option<Entity<AuthEditor>>,
    /// The collection's authorization, which the call sends while it
    /// inherits it. Read again when the tab is shown.
    pub(super) inherited: Option<Inherited>,
    pub(crate) scripts: Option<Entity<ScriptEditor<GrpcScripts>>>,
    pub(super) methods: Option<Entity<SelectState<MethodList>>>,
    pub(super) proto_path: Option<Entity<InputState>>,
    pub(super) import_paths: Vec<Entity<InputState>>,
    pub(super) server_name: Option<Entity<InputState>>,
    pub(super) max_message: Option<Entity<InputState>>,
    pub(super) timeout: Option<Entity<InputState>>,
    pub(crate) definition: DefinitionState,
    /// The settings the current definition was loaded or is loading for.
    pub(super) definition_source: Option<super::definition::DefinitionSource>,
    /// What the URL and metadata resolved to when reflection loaded.
    pub(super) reflected_target: Option<Vec<String>>,
    pub(super) definition_task: Option<Task<()>>,
    /// Invoke once the definition finishes loading.
    pub(super) invoke_when_loaded: bool,
    pub(super) variables: Entity<VariableScope>,
    variable_sessions: EnvironmentSessions,
    pub(crate) response: Entity<GrpcResponse>,
    pub(crate) call: Option<request::GrpcCall>,
    pub(super) call_task: Option<Task<()>>,
    /// A message that could not be sent on the open stream.
    pub(super) send_error: Option<SharedString>,
    pub(super) client: Option<(RequestPreferences, GrpcClient)>,
    split: Entity<ResizableState>,
    /// The call as a grpcurl command, beside the request while open.
    pub(super) code_snippet: SnippetPanel<Self>,
    address: Entity<GrpcAddress>,
    configuration: Entity<GrpcConfiguration>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestSent> for GrpcDraft {}

impl SnippetDraft for GrpcDraft {
    type Request = GrpcRequest;
    const PROGRAM: &'static str = "grpcurl";

    fn request(&self) -> &GrpcRequest {
        &self.request
    }

    fn command(&self, values: &HashMap<String, String>, _: &App) -> String {
        GrpcRequest {
            auth: self.effective_auth(),
            ..self.request.clone()
        }
        .grpcurl_command(values, self.collection_path().as_deref())
    }

    fn variables(&self) -> &Entity<VariableScope> {
        &self.variables
    }

    fn snippet_panel(&mut self) -> &mut SnippetPanel<Self> {
        &mut self.code_snippet
    }

    fn toggle_code_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        code_snippet::toggle(self, window, cx);
        self.redraw(cx);
    }
}

impl GrpcDraft {
    /// Variables resolve from the request's collection environment, its
    /// session values and the active global environment.
    pub fn new(
        request: GrpcRequest,
        location: Option<SavedLocation>,
        sessions: EnvironmentSessions,
        environments: Option<Entity<Environments>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let environment_path = location.as_ref().map(SavedLocation::environment_path);
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

        // Like the HTTP draft, create views that install window listeners
        // only once the tab is visible.
        let owner = cx.weak_entity();
        let response = cx.new(GrpcResponse::new);
        let split = cx.new(|_| ResizableState::default());
        let address = cx.new(|_| GrpcAddress(owner.clone()));
        let configuration = cx.new(|_| GrpcConfiguration(owner));

        Self {
            location,
            name: None,
            saved_request: request.clone(),
            request,
            section: GrpcSection::Message,
            url: None,
            url_completion: None,
            message: None,
            message_completion: None,
            message_vim: None,
            message_json_valid: false,
            metadata: None,
            auth: None,
            inherited: None,
            scripts: None,
            methods: None,
            proto_path: None,
            import_paths: Vec::new(),
            server_name: None,
            max_message: None,
            timeout: None,
            definition: DefinitionState::Idle,
            definition_source: None,
            reflected_target: None,
            definition_task: None,
            invoke_when_loaded: false,
            variables,
            variable_sessions: sessions,
            response,
            call: None,
            call_task: None,
            send_error: None,
            client: None,
            split,
            code_snippet: SnippetPanel::default(),
            address,
            configuration,
            _subscriptions: subscriptions,
        }
    }

    /// Redraw the address bar and sections after the draft changes what they
    /// show. They are cached views, which a notification of the draft alone
    /// does not invalidate; a change by keyboard or from a task would stay
    /// hidden until the next mouse event.
    pub(super) fn redraw(&mut self, cx: &mut Context<Self>) {
        self.address.update(cx, |_, cx| cx.notify());
        self.configuration.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// The authorization the call sends: its own, or its collection's
    /// while it inherits it.
    pub(super) fn effective_auth(&self) -> Auth {
        match (&self.request.auth, &self.inherited) {
            (Auth::Inherit, Some(inherited)) => inherited.auth.clone(),
            (auth, _) => auth.clone(),
        }
    }

    /// Read the collection's authorization again, which another tab may
    /// have changed.
    fn refresh_inherited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.inherited.is_some();

        // Servers may require credentials to answer reflection. A tab shown
        // for the first time loads its services right away instead.
        if self.update_inherited(cx) && shown && self.request.auth.is_inherit() {
            self.schedule_reflection(window, cx);
        }
    }

    /// Whether the collection's authorization changed since it was read.
    fn update_inherited(&mut self, cx: &mut Context<Self>) -> bool {
        let inherited = self.location.as_ref().map(|location| Inherited {
            name: location.collection_name().into(),
            auth: self.variables.read(cx).collection_auth(),
        });
        if inherited == self.inherited {
            return false;
        }

        self.inherited = inherited;
        if let Some(auth) = &self.auth {
            let inherited = self.inherited.clone();
            auth.update(cx, |auth, cx| auth.set_inherited(inherited, cx));
        }

        true
    }

    pub(super) fn auth_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<AuthEditor> {
        if let Some(auth) = &self.auth {
            return auth.clone();
        }

        let inherited = self.inherited.clone();
        let auth = cx.new(|cx| {
            let mut editor = AuthEditor::new(
                self.request.auth.clone(),
                AuthTarget::Grpc,
                self.variables.clone(),
            );
            editor.set_inherited(inherited, cx);
            editor
        });
        self._subscriptions.push(cx.subscribe_in(
            &auth,
            window,
            |this, _, event: &AuthChanged, window, cx| {
                this.request.auth = event.0.clone();
                // Servers may require credentials to answer reflection.
                this.schedule_reflection(window, cx);
                cx.notify();
            },
        ));
        self.auth = Some(auth.clone());

        auth
    }

    /// A name given before the request is saved is an unsaved change too.
    pub fn is_dirty(&self) -> bool {
        self.request != self.saved_request || (self.location.is_none() && self.name.is_some())
    }

    /// Follow the saved request to its current file and name.
    pub fn set_location(&mut self, location: SavedLocation, cx: &mut Context<Self>) {
        let path = Some(location.environment_path());
        let session = self.variable_sessions.for_path(path.as_deref());

        self.variables.update(cx, |scope, cx| {
            scope.path = path;
            scope.session = session;
            scope.changed(cx);
        });

        self.location = Some(location);
        // Invoking reloads the services if the credentials changed.
        self.update_inherited(cx);
        self.redraw(cx);
    }

    /// Name the request before it is saved.
    pub fn set_name(&mut self, name: SharedString, cx: &mut Context<Self>) {
        self.name = Some(name);
        self.redraw(cx);
    }

    pub fn mark_saved(&mut self, request: GrpcRequest, cx: &mut Context<Self>) {
        self.saved_request = request;
        cx.notify();
    }

    /// Copies the call as a grpcurl command, with the variables that resolve
    /// filled in.
    pub fn copy_as_grpcurl(&self, window: &mut Window, cx: &mut App) {
        code_snippet::copy(self, window, cx);
    }

    /// The directory relative `.proto` paths resolve from.
    pub(crate) fn collection_path(&self) -> Option<PathBuf> {
        self.location
            .as_ref()
            .map(|location| location.collection.clone())
    }

    /// The selected method's kind, once the definition describes it.
    pub(crate) fn method_kind(&self) -> Option<MethodKind> {
        match &self.definition {
            DefinitionState::Loaded(definition) => definition
                .method(self.request.method.trim())
                .map(|method| method.kind),
            _ => None,
        }
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.variables.update(cx, |scope, cx| scope.changed(cx));
        self.refresh_inherited(window, cx);

        // Initialize newly activated controls before drawing, as the HTTP
        // draft does, so their setup does not schedule another frame.
        self.url_state(window, cx);
        self.methods_state(window, cx);

        match self.section {
            GrpcSection::Message => {
                self.message_state(window, cx);
            }
            GrpcSection::Metadata => {
                self.metadata_state(window, cx);
            }
            GrpcSection::Auth => {
                self.auth_editor(window, cx)
                    .update(cx, |auth, cx| auth.prepare(window, cx));
            }
            GrpcSection::Definition => self.definition_inputs(window, cx),
            GrpcSection::Scripts => {
                self.script_editor(cx)
                    .update(cx, |scripts, cx| scripts.editor(window, cx));
            }
            GrpcSection::Settings => self.settings_inputs(window, cx),
        }

        // Load the services of a request opened with a URL or `.proto` file.
        if matches!(self.definition, DefinitionState::Idle)
            && self.definition_task.is_none()
            && self.current_source().is_some()
        {
            self.load_definition(false, window, cx);
        }
    }

    /// Puts the cursor in the URL with its text selected, as a browser's
    /// address bar does.
    pub fn focus_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url_state(window, cx);

        url.update(cx, |url, cx| {
            url.select_all(window, cx);
            url.focus(window, cx);
        });
    }

    pub(super) fn url_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let scope = self.variables.clone();

        self.url
            .get_or_insert_with(|| {
                let url = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Enter URL")
                        .default_value(self.request.url.clone())
                });
                self.url_completion = Some(cx.new(|cx| {
                    VariableInput::new(VariableTarget::Input(url.clone()), scope, window, cx)
                }));
                self._subscriptions.push(cx.subscribe_in(
                    &url,
                    window,
                    |this, input, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::Change) {
                            let tls = this.request.tls;
                            this.request.url = input.read(cx).value().to_string();
                            this.request.tls = this.request.uses_tls();
                            this.schedule_reflection(window, cx);

                            // A scheme typed in the URL switches the lock.
                            if this.request.tls != tls {
                                this.redraw(cx);
                            } else {
                                cx.notify();
                            }
                        }
                    },
                ));

                url
            })
            .clone()
    }

    pub(super) fn methods_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<MethodList>> {
        if let Some(methods) = &self.methods {
            return methods.clone();
        }

        let methods =
            cx.new(|cx| SelectState::new(method_list(&[]), None, window, cx).searchable(true));
        self._subscriptions.push(cx.subscribe_in(
            &methods,
            window,
            |this, _, event: &SelectEvent<MethodList>, _, cx| {
                let SelectEvent::Confirm(Some(path)) = event else {
                    return;
                };

                if this.request.method != *path {
                    this.request.method = path.clone();
                    this.redraw(cx);
                }
            },
        ));
        self.methods = Some(methods.clone());
        self.refresh_methods(window, cx);

        methods
    }

    pub(super) fn message_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        if let Some(message) = &self.message {
            return message.clone();
        }

        let message = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .soft_wrap(true)
                .placeholder("Compose message")
                .default_value(self.request.message.clone())
        });
        let scope = self.variables.clone();
        self.message_vim = Some(cx.new(|cx| crate::vim::Vim::new(message.clone(), cx)));
        self.message_completion = Some(cx.new(|cx| {
            VariableInput::new(VariableTarget::Editor(message.clone()), scope, window, cx)
        }));
        self._subscriptions.push(
            cx.subscribe(&message, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    this.message_json_valid =
                        serde_json::from_str::<serde_json::Value>(&value).is_ok();
                    this.request.message = value.to_string();
                    this.send_error = None;
                    cx.notify();
                }
            }),
        );
        self.message_json_valid =
            serde_json::from_str::<serde_json::Value>(&self.request.message).is_ok();
        self.message = Some(message.clone());

        message
    }

    pub(super) fn metadata_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        if let Some(metadata) = &self.metadata {
            return metadata.clone();
        }

        let scope = self.variables.clone();
        let metadata = cx.new(|cx| {
            RequestFields::new("metadata", &self.request.metadata, &[], scope, window, cx)
        });
        self._subscriptions.push(cx.subscribe_in(
            &metadata,
            window,
            |this, _, event: &FieldsChanged, window, cx| {
                let count = Field::enabled(&this.request.metadata).count();
                this.request.metadata = event.0.clone();
                // Servers may require credentials to answer reflection.
                this.schedule_reflection(window, cx);

                // The Metadata tab shows the count.
                if Field::enabled(&this.request.metadata).count() != count {
                    this.redraw(cx);
                } else {
                    cx.notify();
                }
            },
        ));
        self.metadata = Some(metadata.clone());

        metadata
    }

    pub(crate) fn script_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<ScriptEditor<GrpcScripts>> {
        if let Some(scripts) = &self.scripts {
            return scripts.clone();
        }

        let scripts =
            cx.new(|_| ScriptEditor::new(self.request.scripts.clone(), ScriptTarget::Request));
        self._subscriptions.push(cx.subscribe(
            &scripts,
            |this, _, event: &ScriptsChanged<GrpcScripts>, cx| {
                let count = this.script_count();
                this.request.scripts = event.0.clone();

                // The Scripts tab shows the count.
                if this.script_count() != count {
                    this.redraw(cx);
                } else {
                    cx.notify();
                }
            },
        ));
        self.scripts = Some(scripts.clone());

        scripts
    }

    /// How many of the call's scripts are written.
    pub(super) fn script_count(&self) -> usize {
        let scripts = &self.request.scripts;

        [
            &scripts.before_invoke,
            &scripts.on_message,
            &scripts.after_response,
        ]
        .into_iter()
        .filter(|script| !script.is_empty())
        .count()
    }
}

impl Render for GrpcDraft {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let request = v_flex()
            .debug_selector(|| "grpc-draft".into())
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
                    v_resizable("grpc-message-response-split")
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

// Cache the editable controls independently of the response, so streamed
// messages do not redraw the address bar and message editor.
struct GrpcAddress(WeakEntity<GrpcDraft>);

impl Render for GrpcAddress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(
                                crate::request_draft::request_header(
                                    "gRPC",
                                    draft.location.as_ref(),
                                    draft.name.as_ref(),
                                    cx,
                                ),
                            ))
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

struct GrpcConfiguration(WeakEntity<GrpcDraft>);

impl Render for GrpcConfiguration {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                let content = match draft.section {
                    GrpcSection::Message => draft.message_editor(window, cx),
                    GrpcSection::Metadata => draft.metadata_state(window, cx).into_any_element(),
                    GrpcSection::Auth => draft.auth_editor(window, cx).into_any_element(),
                    GrpcSection::Definition => draft.definition_tab(window, cx),
                    GrpcSection::Scripts => draft.script_editor(cx).into_any_element(),
                    GrpcSection::Settings => draft.settings_tab(window, cx),
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
                            .id("grpc-section-content")
                            .debug_selector(|| "grpc-section-content".into())
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(content),
                    )
            })
            .unwrap_or_else(|_| div())
    }
}
