use std::collections::HashMap;

use gpui_kit::component::{
    input::{EditorState, InputEvent, InputState},
    select::{SearchableVec, SelectEvent, SelectState},
    *,
};
use gpui_kit::*;
use request::{Auth, AuthKind};

use super::fields::{Choice, Field, Row, rows};
use crate::variable_input::{VariableInput, VariableTarget};
use crate::variables::VariableScope;

pub(super) type Options = SearchableVec<SharedString>;

/// The authorization after an edit.
pub(crate) struct AuthChanged(pub Auth);

/// What the authorization belongs to, which decides the kinds it offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthTarget {
    Http,
    WebSocket,
    Grpc,
    /// A collection's, which its requests inherit.
    Collection,
}

impl AuthTarget {
    pub(super) fn offers(self, kind: AuthKind) -> bool {
        match self {
            Self::Collection => kind != AuthKind::Inherit,
            Self::Grpc => kind.supports_grpc(),
            Self::Http | Self::WebSocket => true,
        }
    }

    /// Whether credentials can go in a query. A gRPC call has none.
    pub(super) fn has_query(self) -> bool {
        self != Self::Grpc
    }
}

/// The collection a request inherits its authorization from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Inherited {
    pub name: SharedString,
    pub auth: Auth,
}

pub(super) struct FieldInput {
    pub target: VariableTarget,
    /// None for a hidden secret, whose masked text cannot show variables
    /// where they are.
    pub completion: Option<Entity<VariableInput>>,
}

/// The Auth tab of a request or collection: a kind of authorization and its
/// fields. Values typed for one kind stay while another is chosen.
pub(crate) struct AuthEditor {
    pub(super) auth: Auth,
    pub(super) target: AuthTarget,
    /// Where an inheriting request's authorization comes from. None for a
    /// request outside a collection.
    pub(super) inherited: Option<Inherited>,
    pub(super) scope: Entity<VariableScope>,
    /// The authorizations of the other kinds, as they were last edited.
    stash: HashMap<AuthKind, Auth>,
    pub(super) kinds: Option<Entity<SelectState<Options>>>,
    pub(super) inputs: HashMap<(AuthKind, Field), FieldInput>,
    pub(super) choices: HashMap<(AuthKind, Choice), Entity<SelectState<Options>>>,
    /// Getting a new OAuth 2.0 access token.
    pub(super) token_task: Option<Task<()>>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AuthChanged> for AuthEditor {}

impl AuthEditor {
    pub(crate) fn new(auth: Auth, target: AuthTarget, scope: Entity<VariableScope>) -> Self {
        // A collection has no parent to inherit from.
        let auth = match auth {
            Auth::Inherit if target == AuthTarget::Collection => Auth::None,
            auth => auth,
        };

        Self {
            auth,
            target,
            inherited: None,
            scope,
            stash: HashMap::new(),
            kinds: None,
            inputs: HashMap::new(),
            choices: HashMap::new(),
            token_task: None,
            _subscriptions: Vec::new(),
        }
    }

    pub(crate) fn set_inherited(&mut self, inherited: Option<Inherited>, cx: &mut Context<Self>) {
        if self.inherited != inherited {
            self.inherited = inherited;
            cx.notify();
        }
    }

    /// Create the controls of the chosen kind before they are drawn.
    pub(crate) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.kinds(window, cx);

        for row in rows(&self.auth, self.target.has_query()) {
            match row {
                Row::Text(field) => {
                    self.input(field, window, cx);
                }
                Row::Choice(choice) => {
                    self.choice(choice, window, cx);
                }
                _ => {}
            }
        }
    }

    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(AuthChanged(self.auth.clone()));
        cx.notify();
    }

    /// Switch to another kind, with the values it had when last chosen.
    fn set_kind(&mut self, kind: AuthKind, window: &mut Window, cx: &mut Context<Self>) {
        if kind == self.auth.kind() {
            return;
        }

        let auth = self.stash.remove(&kind).unwrap_or_else(|| kind.new_auth());
        let previous = std::mem::replace(&mut self.auth, auth);
        self.stash.insert(previous.kind(), previous);
        // A token request belongs to the OAuth 2.0 fields it started from.
        self.token_task = None;

        self.prepare(window, cx);
        self.changed(cx);
    }

    pub(super) fn kinds(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Options>> {
        if let Some(kinds) = &self.kinds {
            return kinds.clone();
        }

        let offered: Vec<AuthKind> = AuthKind::ALL
            .into_iter()
            .filter(|kind| self.target.offers(*kind))
            .collect();
        let labels: Vec<SharedString> = offered.iter().map(|kind| kind.label().into()).collect();
        let selected = offered
            .iter()
            .position(|kind| *kind == self.auth.kind())
            .map(IndexPath::new);
        let kinds = cx.new(|cx| SelectState::new(SearchableVec::new(labels), selected, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &kinds,
            window,
            move |this, _, event: &SelectEvent<Options>, window, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some(kind) = offered.iter().find(|kind| kind.label() == label.as_ref())
                {
                    this.set_kind(*kind, window, cx);
                }
            },
        ));
        self.kinds = Some(kinds.clone());

        kinds
    }

    pub(super) fn input(
        &mut self,
        field: Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> &FieldInput {
        let kind = self.auth.kind();
        let key = (kind, field);
        if !self.inputs.contains_key(&key) {
            let text = field.text(&self.auth);
            let target = if field.multiline() {
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(if field == Field::PrivateKey {
                            "text"
                        } else {
                            "json"
                        })
                        .line_number(false)
                        .soft_wrap(true)
                        .placeholder(field.placeholder(kind))
                        .default_value(text)
                });
                self._subscriptions.push(cx.subscribe(
                    &editor,
                    move |this, editor, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.set_text(field, editor.read(cx).value().to_string(), cx);
                        }
                    },
                ));
                VariableTarget::Editor(editor)
            } else {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(field.placeholder(kind))
                        .masked(field.secret())
                        .default_value(text)
                });
                self._subscriptions.push(cx.subscribe(
                    &input,
                    move |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.set_text(field, input.read(cx).value().to_string(), cx);
                        }
                    },
                ));
                VariableTarget::Input(input)
            };
            let scope = self.scope.clone();
            let completion = (!field.secret())
                .then(|| cx.new(|cx| VariableInput::new(target.clone(), scope, window, cx)));

            self.inputs.insert(key, FieldInput { target, completion });
        }

        &self.inputs[&key]
    }

    fn set_text(&mut self, field: Field, value: String, cx: &mut Context<Self>) {
        if let Some(text) = field.text_mut(&mut self.auth)
            && *text != value
        {
            *text = value;
            self.changed(cx);
        }
    }

    pub(super) fn choice(
        &mut self,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Options>> {
        let key = (self.auth.kind(), choice);
        if let Some(select) = self.choices.get(&key) {
            return select.clone();
        }

        let options = choice.options();
        let selected = choice
            .selected(&self.auth)
            .and_then(|label| options.iter().position(|option| *option == label))
            .map(IndexPath::new);
        let labels: Vec<SharedString> = options.into_iter().map(SharedString::from).collect();
        let select =
            cx.new(|cx| SelectState::new(SearchableVec::new(labels), selected, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &select,
            window,
            move |this, _, event: &SelectEvent<Options>, window, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && choice.selected(&this.auth) != Some(label.as_ref())
                {
                    choice.select(&mut this.auth, label);
                    // The choice may show other fields.
                    this.prepare(window, cx);
                    this.changed(cx);
                }
            },
        ));
        self.choices.insert(key, select.clone());

        select
    }

    /// Show a value set outside its input, such as a new access token.
    pub(super) fn set_field(
        &mut self,
        field: Field,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.inputs.get(&(self.auth.kind(), field)) {
            match &input.target {
                VariableTarget::Input(input) => {
                    input.update(cx, |input, cx| input.set_value(value.clone(), window, cx))
                }
                VariableTarget::Editor(editor) => {
                    editor.update(cx, |editor, cx| editor.set_value(value.clone(), window, cx))
                }
            }
        }

        self.set_text(field, value, cx);
    }
}
