use std::time::{Duration, SystemTime};

use environment::EnvironmentSessions;
use futures::{FutureExt as _, StreamExt as _};
use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    resizable::{ResizableState, resizable_panel, v_resizable},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use preferences::Preferences;
use request::{Auth, WebSocketConnection, WebSocketEvent, WebSocketEventKind, WebSocketRequest};

use super::message_log::MessageLog;
use crate::actions::SendRequest;
use crate::auth_editor::{AuthChanged, AuthEditor, AuthTarget, Inherited};
use crate::request_draft::{FieldsChanged, RequestFields, RequestLocation, request_header};
use crate::variable_input::{VariableInput, VariableTarget, with_variables};
use crate::variables::VariableScope;
use crate::{Environments, RequestSent};

/// The most events shown per update. A fast stream is drawn in batches
/// instead of once for every message.
const EVENT_BATCH: usize = 512;

/// The pause after each batch, which lets the window draw and handle input
/// while a stream keeps the queue full.
const BATCH_PAUSE: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    /// Closing was requested; waiting for the server to acknowledge.
    Closing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebSocketSection {
    Message,
    Params,
    Auth,
    Headers,
    Settings,
}

/// An editable WebSocket request owned by one tab, and its connection.
pub struct WebSocketDraft {
    /// Unsaved drafts have no location.
    pub location: Option<RequestLocation>,
    /// The name given to the request in its tab before it is saved.
    pub name: Option<SharedString>,
    pub request: WebSocketRequest,
    saved_request: WebSocketRequest,
    pub(crate) section: WebSocketSection,
    pub(crate) state: ConnectionState,
    pub(crate) url: Option<Entity<InputState>>,
    url_completion: Option<Entity<VariableInput>>,
    params: Option<Entity<RequestFields>>,
    headers: Option<Entity<RequestFields>>,
    handshake_headers: Vec<(String, String)>,
    auth: Option<Entity<AuthEditor>>,
    /// The collection's authorization, which the request sends while it
    /// inherits it. Read again when the tab is shown.
    inherited: Option<Inherited>,
    pub(crate) message: Option<Entity<EditorState>>,
    message_vim: Option<Entity<crate::vim::Vim>>,
    message_completion: Option<Entity<VariableInput>>,
    message_json_valid: bool,
    message_task: Option<Task<()>>,
    pub(super) timeout: Option<Entity<InputState>>,
    variables: Entity<VariableScope>,
    variable_sessions: EnvironmentSessions,
    pub(crate) log: Entity<MessageLog>,
    split: Entity<ResizableState>,
    connection: Option<WebSocketConnection>,
    /// The request as it was when it started connecting. History keeps it
    /// once it connects.
    connecting: Option<RequestSent>,
    events: Option<Task<()>>,
    address: Entity<WebSocketAddress>,
    configuration: Entity<WebSocketConfiguration>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestSent> for WebSocketDraft {}

impl WebSocketDraft {
    /// Variables resolve from the request's collection environment, its
    /// session values and the active global environment.
    pub fn new(
        request: WebSocketRequest,
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

        // Editors install window listeners, so they wait until the tab is shown.
        let owner = cx.weak_entity();
        let log = cx.new(MessageLog::new);
        let split = cx.new(|_| ResizableState::default());
        let address = cx.new(|_| WebSocketAddress(owner.clone()));
        let configuration = cx.new(|_| WebSocketConfiguration(owner));

        Self {
            location,
            name: None,
            handshake_headers: request::websocket_handshake_headers(
                &request.url,
                &request.headers,
                &request.auth,
            ),
            auth: None,
            inherited: None,
            saved_request: request.clone(),
            request,
            section: WebSocketSection::Message,
            state: ConnectionState::Disconnected,
            url: None,
            url_completion: None,
            params: None,
            headers: None,
            message: None,
            message_vim: None,
            message_completion: None,
            message_json_valid: false,
            message_task: None,
            timeout: None,
            variables,
            variable_sessions: sessions,
            log,
            split,
            connection: None,
            connecting: None,
            events: None,
            address,
            configuration,
            _subscriptions: subscriptions,
        }
    }

    /// The authorization the request sends: its own, or its collection's
    /// while it inherits it.
    fn effective_auth(&self) -> Auth {
        match (&self.request.auth, &self.inherited) {
            (Auth::Inherit, Some(inherited)) => inherited.auth.clone(),
            (auth, _) => auth.clone(),
        }
    }

    /// Read the collection's authorization again, which another tab may
    /// have changed.
    fn refresh_inherited(&mut self, cx: &mut Context<Self>) {
        let inherited = self.location.as_ref().map(|location| Inherited {
            name: location.collection.clone(),
            auth: self.variables.read(cx).collection_auth(),
        });
        if inherited == self.inherited {
            return;
        }

        self.inherited = inherited;
        if let Some(auth) = &self.auth {
            let inherited = self.inherited.clone();
            auth.update(cx, |auth, cx| auth.set_inherited(inherited, cx));
        }
        self.refresh_handshake_headers(cx);
    }

    fn auth_editor(&mut self, cx: &mut Context<Self>) -> Entity<AuthEditor> {
        if let Some(auth) = &self.auth {
            return auth.clone();
        }

        let inherited = self.inherited.clone();
        let auth = cx.new(|cx| {
            let mut editor = AuthEditor::new(
                self.request.auth.clone(),
                AuthTarget::WebSocket,
                self.variables.clone(),
            );
            editor.set_inherited(inherited, cx);
            editor
        });
        self._subscriptions
            .push(cx.subscribe(&auth, |this, _, event: &AuthChanged, cx| {
                this.request.auth = event.0.clone();
                this.refresh_handshake_headers(cx);
                // The Headers tab counts the handshake's headers.
                this.notify_controls(cx);
            }));
        self.auth = Some(auth.clone());

        auth
    }

    /// A name given before the request is saved is an unsaved change too.
    pub fn is_dirty(&self) -> bool {
        self.request != self.saved_request || (self.location.is_none() && self.name.is_some())
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
        self.notify_controls(cx);
    }

    /// Name the request before it is saved.
    pub fn set_name(&mut self, name: SharedString, cx: &mut Context<Self>) {
        self.name = Some(name);
        self.notify_controls(cx);
    }

    pub fn mark_saved(&mut self, request: WebSocketRequest, cx: &mut Context<Self>) {
        self.saved_request = request;
        cx.notify();
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.variables.update(cx, |scope, cx| scope.changed(cx));
        self.refresh_inherited(cx);

        // Initialize newly shown controls before drawing, as request drafts do.
        self.url_state(window, cx);

        match self.section {
            WebSocketSection::Message => {
                self.message_state(window, cx);
            }
            WebSocketSection::Params | WebSocketSection::Headers => {
                self.fields_state(window, cx);
            }
            WebSocketSection::Auth => {
                self.auth_editor(cx)
                    .update(cx, |auth, cx| auth.prepare(window, cx));
            }
            WebSocketSection::Settings => {
                self.timeout_state(window, cx);
            }
        }

        self.log.update(cx, |log, cx| log.prepare(window, cx));
    }

    /// Connect, or send the composed message once connected.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.state {
            ConnectionState::Disconnected => self.connect(window, cx),
            ConnectionState::Connected => self.send_message(cx),
            ConnectionState::Connecting | ConnectionState::Closing => {}
        }
    }

    pub fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state != ConnectionState::Disconnected {
            return;
        }

        self.prepare(window, cx);

        let variables = self.variables.read(cx).request_variables(cx);
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        // History keeps the authorization that was sent, inherited or not.
        let recorded = WebSocketRequest {
            auth: variables.effective_auth(&self.request.auth),
            ..self.request.clone()
        };
        let (connection, mut events) =
            WebSocketConnection::open(self.request.clone(), variables, &preferences);
        self.connecting = Some(RequestSent {
            record: request_history::Record::sent(recorded),
            sent_at: SystemTime::now(),
        });

        self.connection = Some(connection);
        self.set_state(ConnectionState::Connecting, cx);

        self.events = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let mut batch = vec![event];
                while batch.len() < EVENT_BATCH
                    && let Some(Some(event)) = events.next().now_or_never()
                {
                    batch.push(event);
                }

                if this.update(cx, |this, cx| this.receive(batch, cx)).is_err() {
                    return;
                }

                cx.background_executor().timer(BATCH_PAUSE).await;
            }
        }));
    }

    /// Close the connection, or stop connecting.
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        match self.state {
            ConnectionState::Connecting => {
                self.connection = None;
                self.connecting = None;
                self.events = None;
                self.set_state(ConnectionState::Disconnected, cx);
            }
            ConnectionState::Connected => {
                if let Some(connection) = &mut self.connection {
                    connection.close();
                }
                self.set_state(ConnectionState::Closing, cx);
            }
            ConnectionState::Disconnected | ConnectionState::Closing => {}
        }
    }

    pub fn send_message(&mut self, cx: &mut Context<Self>) {
        let Some(connection) = self
            .connection
            .as_ref()
            .filter(|_| self.state == ConnectionState::Connected)
        else {
            return;
        };

        let variables = self.variables.read(cx).request_variables(cx);
        connection.send(self.request.message.clone(), variables);
    }

    pub(super) fn receive(&mut self, events: Vec<WebSocketEvent>, cx: &mut Context<Self>) {
        let mut state = self.state;

        for event in &events {
            match event.kind {
                WebSocketEventKind::Connected(_) if state == ConnectionState::Connecting => {
                    state = ConnectionState::Connected;

                    if let Some(sent) = self.connecting.take() {
                        cx.emit(sent);
                    }
                }
                WebSocketEventKind::Closed(_) | WebSocketEventKind::Failed(_) => {
                    state = ConnectionState::Disconnected;
                    self.connecting = None;
                }
                _ => {}
            }
        }

        self.log.update(cx, |log, cx| log.push(events, cx));

        if state == ConnectionState::Disconnected {
            self.connection = None;
        }
        if state != self.state {
            self.set_state(state, cx);
        }
    }

    fn set_state(&mut self, state: ConnectionState, cx: &mut Context<Self>) {
        self.state = state;
        self.log.update(cx, |log, cx| log.set_state(state, cx));
        self.notify_controls(cx);
    }

    /// Redraw the cached URL bar and sections. Notifying the draft alone
    /// redraws only its uncached content, which is enough while handling
    /// their own input, but not for changes that arrive later.
    fn notify_controls(&self, cx: &mut Context<Self>) {
        self.address.update(cx, |_, cx| cx.notify());
        self.configuration.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn refresh_handshake_headers(&mut self, cx: &mut Context<Self>) {
        self.handshake_headers = request::websocket_handshake_headers(
            &self.request.url,
            &self.request.headers,
            &self.effective_auth(),
        );

        if let Some(headers) = &self.headers {
            headers.update(cx, |headers, cx| {
                headers.set_generated_headers(&self.handshake_headers, cx)
            });
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

    pub(crate) fn url_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let scope = self.variables.clone();

        self.url
            .get_or_insert_with(|| {
                let url = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Enter a ws:// or wss:// URL")
                        .default_value(self.request.url.clone())
                });
                self.url_completion = Some(cx.new(|cx| {
                    VariableInput::new(VariableTarget::Input(url.clone()), scope, window, cx)
                }));
                self._subscriptions.push(cx.subscribe(
                    &url,
                    |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.request.url = input.read(cx).value().to_string();
                            this.refresh_handshake_headers(cx);
                            cx.notify();
                        }
                    },
                ));

                url
            })
            .clone()
    }

    fn fields_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        let scope = self.variables.clone();
        let is_headers = self.section == WebSocketSection::Headers;
        let slot = if is_headers {
            &mut self.headers
        } else {
            &mut self.params
        };

        if slot.is_none() {
            let (id, values, generated) = if is_headers {
                (
                    "headers",
                    self.request.headers.as_slice(),
                    self.handshake_headers.as_slice(),
                )
            } else {
                ("params", self.request.query.as_slice(), &[][..])
            };
            let fields = cx.new(|cx| RequestFields::new(id, values, generated, scope, window, cx));
            let subscription = cx.subscribe(&fields, move |this, _, event: &FieldsChanged, cx| {
                if is_headers {
                    this.request.headers = event.0.clone();
                    this.refresh_handshake_headers(cx);
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

    pub(crate) fn message_state(
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
                .placeholder("Compose a message")
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
                    this.request.message = value.to_string();
                    this.validate_message(value, cx);
                    cx.notify();
                }
            }),
        );
        self.message = Some(message.clone());
        self.validate_message(message.read(cx).value(), cx);

        message
    }

    fn validate_message(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.message_json_valid = false;

        // A new edit drops the previous task, so an old result cannot enable
        // Format or overwrite a more recent message.
        let task = cx
            .background_executor()
            .spawn(async move { serde_json::from_str::<serde_json::Value>(&text).is_ok() });
        self.message_task = Some(cx.spawn(async move |this, cx| {
            let valid = task.await;
            let _ = this.update(cx, |this, cx| {
                this.message_json_valid = valid;
                this.message_task = None;
                this.notify_controls(cx);
            });
        }));
    }

    fn format_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.message_json_valid || self.message_task.is_some() {
            return;
        }

        let Some(message) = &self.message else { return };
        let text = message.read(cx).value();
        let source = text.clone();
        let task = cx.background_executor().spawn(async move {
            serde_json::from_str::<serde_json::Value>(&text)
                .and_then(|value| serde_json::to_string_pretty(&value))
        });
        self.message_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.message_task = None;

                if let Ok(text) = result
                    && let Some(message) = &this.message
                    && message.read(cx).value() == source
                {
                    message.update(cx, |message, cx| message.replace_all(text, window, cx));
                }

                this.notify_controls(cx);
            });
        }));
        cx.notify();
    }

    fn url_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let url = self.url_state(window, cx);
        let state = self.state;

        h_flex()
            .flex_none()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "websocket-url".into())
                    .flex_1()
                    .min_w_0()
                    .child(with_variables(
                        self.url_completion.as_ref().unwrap(),
                        Input::new(&url).aria_label("WebSocket URL"),
                    )),
            )
            .child(
                Button::new("websocket-connect")
                    .debug_selector(|| "websocket-connect".into())
                    .min_w_24()
                    .flex_none()
                    .map(|button| match state {
                        ConnectionState::Disconnected => button.primary(),
                        _ => button,
                    })
                    .label(match state {
                        ConnectionState::Disconnected => "Connect",
                        ConnectionState::Connecting => "Cancel",
                        ConnectionState::Connected | ConnectionState::Closing => "Disconnect",
                    })
                    .disabled(state == ConnectionState::Closing)
                    .when(state == ConnectionState::Disconnected, |button| {
                        button.tooltip_with_action("Connect", &SendRequest, Some("Workspace"))
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.state == ConnectionState::Disconnected {
                            this.connect(window, cx);
                        } else {
                            this.disconnect(cx);
                        }
                    })),
            )
    }

    fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sections = [
            ("Message", WebSocketSection::Message, 0),
            ("Params", WebSocketSection::Params, self.request.query.len()),
            ("Auth", WebSocketSection::Auth, 0),
            (
                "Headers",
                WebSocketSection::Headers,
                self.request.headers.len() + self.handshake_headers.len(),
            ),
            ("Settings", WebSocketSection::Settings, 0),
        ];

        Tabs::new("websocket-sections")
            .flex_none()
            .flex()
            .gap_1()
            .children(sections.into_iter().map(|(label, section, count)| {
                let selected = section == self.section;

                Tab::new(label)
                    .debug_selector(move || format!("websocket-section-{label}"))
                    .selected(selected)
                    .accessibility_label(label)
                    .flex_none()
                    .h_8()
                    .px_2()
                    .gap_1()
                    .rounded(cx.theme().radius_tokens().md)
                    .text_color(cx.theme().muted_foreground)
                    .when(selected, |this| {
                        this.bg(cx.theme().muted).text_color(cx.theme().foreground)
                    })
                    .hover(|this| this.bg(cx.theme().muted))
                    .child(label)
                    .when(count > 0, |this| {
                        this.child(crate::section_count::section_count(count, selected, cx))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.section = section;
                        this.prepare(window, cx);
                        cx.notify();
                    }))
            }))
    }

    fn composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let message = self.message_state(window, cx);
        let vim = self.message_vim.clone().unwrap();
        let mouse_vim = vim.clone();
        let connected = self.state == ConnectionState::Connected;

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "websocket-message".into())
                    .track_focus(&vim.focus_handle(cx))
                    .capture_any_mouse_down(move |_, _, cx| {
                        mouse_vim.update(cx, |vim, _| vim.mouse_down());
                    })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        with_variables(
                            self.message_completion.as_ref().unwrap(),
                            Editor::new(&message)
                                .h_full()
                                .appearance(false)
                                .bordered(false)
                                .bg(cx
                                    .theme()
                                    .highlight_theme
                                    .style
                                    .editor_background
                                    .unwrap_or_else(|| cx.theme().input_background()))
                                .text_sm()
                                .aria_label("WebSocket message"),
                        )
                        .h_full(),
                    )
                    .child(crate::vim::cursor(&vim)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(div().flex_1())
                    .child(vim)
                    .child(
                        Button::new("format-websocket-message")
                            .debug_selector(|| "format-websocket-message".into())
                            .ghost()
                            .small()
                            .label("Format")
                            .disabled(!self.message_json_valid || self.message_task.is_some())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format_message(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("send-websocket-message")
                            .debug_selector(|| "send-websocket-message".into())
                            .primary()
                            .small()
                            .min_w_20()
                            .label("Send")
                            .disabled(!connected)
                            .tooltip_with_action("Send message", &SendRequest, Some("Workspace"))
                            .on_click(cx.listener(|this, _, _, cx| this.send_message(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for WebSocketDraft {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "websocket-draft".into())
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
                    v_resizable("websocket-split")
                        .with_state(&self.split)
                        .child(
                            resizable_panel()
                                .size(rems(16.).to_pixels(window.rem_size()))
                                .size_range(
                                    rems(10.).to_pixels(window.rem_size())
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
                                .child(self.log.clone()),
                        ),
                ),
            )
    }
}

// Like the request draft, cache the editing controls separately from the
// message log, so a busy stream does not redraw the URL bar and composer.
struct WebSocketAddress(WeakEntity<WebSocketDraft>);

impl Render for WebSocketAddress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(request_header(
                        "WS",
                        draft.location.as_ref(),
                        draft.name.as_ref(),
                        cx,
                    ))
                    .child(draft.url_bar(window, cx))
            })
            .unwrap_or_else(|_| div())
    }
}

struct WebSocketConfiguration(WeakEntity<WebSocketDraft>);

impl Render for WebSocketConfiguration {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                let content = match draft.section {
                    WebSocketSection::Message => draft.composer(window, cx),
                    WebSocketSection::Params | WebSocketSection::Headers => {
                        draft.fields_state(window, cx).into_any_element()
                    }
                    WebSocketSection::Auth => draft.auth_editor(cx).into_any_element(),
                    WebSocketSection::Settings => draft.settings(window, cx),
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
                            .id("websocket-section-content")
                            .debug_selector(|| "websocket-section-content".into())
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(content),
                    )
            })
            .unwrap_or_else(|_| div())
    }
}
