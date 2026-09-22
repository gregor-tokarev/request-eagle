use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{ApiKeyLocation, Authentication};

use super::draft::RequestDraft;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthenticationKind {
    None,
    Basic,
    Bearer,
    ApiKey,
}

impl AuthenticationKind {
    fn label(self) -> &'static str {
        match self {
            Self::None => "No authentication",
            Self::Basic => "Basic",
            Self::Bearer => "Bearer token",
            Self::ApiKey => "API key",
        }
    }
}

struct AuthenticationChanged(Authentication);

pub(super) struct AuthenticationEditor {
    kind: AuthenticationKind,
    location: ApiKeyLocation,
    username: Entity<InputState>,
    password: Entity<InputState>,
    token: Entity<InputState>,
    key_name: Entity<InputState>,
    key_value: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AuthenticationChanged> for AuthenticationEditor {}

impl AuthenticationEditor {
    fn new(authentication: &Authentication, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (kind, username, password, token, key_name, key_value, location) = match authentication
        {
            Authentication::None => (
                AuthenticationKind::None,
                "",
                "",
                "",
                "",
                "",
                ApiKeyLocation::Header,
            ),
            Authentication::Basic { username, password } => (
                AuthenticationKind::Basic,
                username.as_str(),
                password.as_str(),
                "",
                "",
                "",
                ApiKeyLocation::Header,
            ),
            Authentication::Bearer { token } => (
                AuthenticationKind::Bearer,
                "",
                "",
                token.as_str(),
                "",
                "",
                ApiKeyLocation::Header,
            ),
            Authentication::ApiKey {
                name,
                value,
                location,
            } => (
                AuthenticationKind::ApiKey,
                "",
                "",
                "",
                name.as_str(),
                value.as_str(),
                *location,
            ),
        };
        let username = cx.new(|cx| InputState::new(window, cx).default_value(username));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(password)
                .masked(true)
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(token)
                .masked(true)
        });
        let key_name = cx.new(|cx| InputState::new(window, cx).default_value(key_name));
        let key_value = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(key_value)
                .masked(true)
        });
        let mut subscriptions = Vec::new();

        for input in [&username, &password, &token, &key_name, &key_value] {
            subscriptions.push(cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.changed(cx);
                }
            }));
        }

        Self {
            kind,
            location,
            username,
            password,
            token,
            key_name,
            key_value,
            _subscriptions: subscriptions,
        }
    }

    fn changed(&self, cx: &mut Context<Self>) {
        let authentication = match self.kind {
            AuthenticationKind::None => Authentication::None,
            AuthenticationKind::Basic => Authentication::Basic {
                username: self.username.read(cx).value().to_string(),
                password: self.password.read(cx).value().to_string(),
            },
            AuthenticationKind::Bearer => Authentication::Bearer {
                token: self.token.read(cx).value().to_string(),
            },
            AuthenticationKind::ApiKey => Authentication::ApiKey {
                name: self.key_name.read(cx).value().to_string(),
                value: self.key_value.read(cx).value().to_string(),
                location: self.location,
            },
        };
        cx.emit(AuthenticationChanged(authentication));
        cx.notify();
    }
}

impl RequestDraft {
    pub(super) fn authentication_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<AuthenticationEditor> {
        if let Some(editor) = &self.authentication_editor {
            return editor.clone();
        }

        let editor =
            cx.new(|cx| AuthenticationEditor::new(&self.request.authentication, window, cx));
        self._subscriptions.push(cx.subscribe(
            &editor,
            |this, _, event: &AuthenticationChanged, cx| {
                this.request.authentication = event.0.clone();
                this.refresh_generated_headers(cx);
                cx.notify();
            },
        ));
        self.authentication_editor = Some(editor.clone());

        editor
    }

    pub(super) fn authentication(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.authentication_state(window, cx).into_any_element()
    }
}

fn field(
    label: &'static str,
    selector: &'static str,
    input: &Entity<InputState>,
    secret: bool,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(div().text_color(cx.theme().muted_foreground).child(label))
        .child(
            div().debug_selector(move || selector.into()).child(
                Input::new(input)
                    .aria_label(label)
                    .when(secret, |input| input.mask_toggle()),
            ),
        )
        .into_any_element()
}

impl Render for AuthenticationEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity().downgrade();
        let kind = self.kind;
        let location = self.location;

        v_flex()
            .debug_selector(|| "request-authentication".into())
            .gap_3()
            .child(
                Button::new("authentication-type")
                    .debug_selector(|| "authentication-type".into())
                    .label(kind.label())
                    .justify_between()
                    .child(Icon::new(IconName::ChevronDown).size(px(12.)))
                    .dropdown_menu(move |mut menu, _, _| {
                        for option in [AuthenticationKind::None, AuthenticationKind::Basic, AuthenticationKind::Bearer, AuthenticationKind::ApiKey] {
                            let editor = editor.clone();
                            menu = menu.item(PopupMenuItem::new(option.label())
                                .checked(option == kind)
                                .on_click(move |_, _, cx| {
                                    let _ = editor.update(cx, |editor, cx| {
                                        editor.kind = option;
                                        editor.changed(cx);
                                    });
                                }));
                        }

                        menu
                    }),
            )
            .when(kind == AuthenticationKind::Basic, |view| {
                view.child(field("Username", "authentication-username", &self.username, false, cx))
                    .child(field("Password", "authentication-password", &self.password, true, cx))
            })
            .when(kind == AuthenticationKind::Bearer, |view| {
                view.child(field("Token", "authentication-token", &self.token, true, cx))
            })
            .when(kind == AuthenticationKind::ApiKey, |view| {
                let editor = cx.entity().downgrade();

                view.child(field("Key", "authentication-key-name", &self.key_name, false, cx))
                    .child(field("Value", "authentication-key-value", &self.key_value, true, cx))
                    .child(Button::new("authentication-key-location")
                        .debug_selector(|| "authentication-key-location".into())
                        .label(match location { ApiKeyLocation::Header => "Add to headers", ApiKeyLocation::Query => "Add to query parameters" })
                        .child(Icon::new(IconName::ChevronDown).size(px(12.)))
                        .dropdown_menu(move |mut menu, _, _| {
                            for (option, label) in [(ApiKeyLocation::Header, "Headers"), (ApiKeyLocation::Query, "Query parameters")] {
                                let editor = editor.clone();
                                menu = menu.item(PopupMenuItem::new(label)
                                    .checked(option == location)
                                    .on_click(move |_, _, cx| {
                                        let _ = editor.update(cx, |editor, cx| {
                                            editor.location = option;
                                            editor.changed(cx);
                                        });
                                    }));
                            }

                            menu
                        }))
            })
            .when(kind != AuthenticationKind::None, |view| {
                view.child(div().text_color(cx.theme().muted_foreground).child(
                    "Explicit headers and query parameters with the same name take precedence.",
                ))
                .child(div().text_color(cx.theme().muted_foreground).child(
                    "Values are stored with this request. Use {{variable}} placeholders for environment values.",
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Modifiers, TestAppContext};
    use request::{ApiKeyLocation, Authentication};

    use super::{AuthenticationKind, RequestDraft};

    #[gpui_kit::test]
    fn authentication_edits_refresh_masked_header_previews(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            request_eagle_theme::init(cx);
        });
        let (draft, cx) = cx.add_window_view(|window, cx| {
            let mut draft = RequestDraft::from_saved(
                "Authenticated request".into(),
                "API".into(),
                request::HttpRequest {
                    path: "http://sam:pass@example.com/".into(),
                    authentication: Authentication::ApiKey {
                        name: "Authorization".into(),
                        value: "saved-secret".into(),
                        location: ApiKeyLocation::Header,
                    },
                    ..Default::default()
                },
            );
            draft.prepare(window, cx);

            draft
        });
        cx.read(|cx| {
            assert!(
                draft
                    .read(cx)
                    .generated_headers
                    .contains(&("Authorization".into(), "[hidden]".into(),))
            );
        });
        cx.update(|window, _| window.refresh());
        let tab = cx.debug_bounds("request-section-Authentication").unwrap();
        cx.simulate_click(tab.center(), Modifiers::default());
        cx.update(|window, _| window.refresh());
        let key_name = cx.debug_bounds("authentication-key-name").unwrap();
        cx.simulate_click(key_name.center(), Modifiers::default());
        cx.simulate_keystrokes("secondary-a");
        cx.simulate_input("X-API-Key");
        cx.read(|cx| {
            let headers = &draft.read(cx).generated_headers;
            assert!(headers.contains(&("X-API-Key".into(), "[hidden]".into())));
            assert!(headers.contains(&("Authorization".into(), "Basic c2FtOnBhc3M=".into())));
            assert!(
                headers
                    .iter()
                    .all(|(_, value)| !value.contains("saved-secret"))
            );
        });
        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("request-section-Headers-count-5").is_some());
        let editor = cx.read(|cx| {
            draft
                .read(cx)
                .authentication_editor
                .as_ref()
                .unwrap()
                .clone()
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.kind = AuthenticationKind::Bearer;
                editor.changed(cx);
            })
        });
        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("request-section-Headers-count-4").is_some());
        let token = cx.debug_bounds("authentication-token").unwrap();
        cx.simulate_click(token.center(), Modifiers::default());
        cx.simulate_input("edited-secret");
        cx.read(|cx| {
            let state = draft.read(cx);
            assert_eq!(
                state.request.authentication,
                Authentication::Bearer {
                    token: "edited-secret".into(),
                }
            );
            assert!(
                state
                    .generated_headers
                    .contains(&("Authorization".into(), "Bearer [hidden]".into(),))
            );
            assert!(state.generated_headers.iter().all(|(name, value)| {
                name != "X-API-Key"
                    && !value.contains("edited-secret")
                    && !value.starts_with("Basic ")
            }));
            assert!(editor.read(cx).token.read(cx).presentation().is_masked());
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.kind = AuthenticationKind::None;
                editor.changed(cx);
            })
        });
        cx.read(|cx| {
            assert!(
                draft
                    .read(cx)
                    .generated_headers
                    .contains(&("Authorization".into(), "Basic c2FtOnBhc3M=".into(),))
            );
        });
    }

    #[gpui_kit::test]
    fn saved_authentication_opens_masked_and_edits_update_the_request(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            request_eagle_theme::init(cx);
        });
        let (draft, cx) = cx.add_window_view(|window, cx| {
            let mut draft = RequestDraft::from_saved(
                "Authenticated request".into(),
                "API".into(),
                request::HttpRequest {
                    authentication: Authentication::Basic {
                        username: "saved-user".into(),
                        password: "saved-password".into(),
                    },
                    ..Default::default()
                },
            );
            draft.prepare(window, cx);

            draft
        });
        cx.update(|window, _| window.refresh());
        let tab = cx.debug_bounds("request-section-Authentication").unwrap();
        cx.simulate_click(tab.center(), Modifiers::default());
        cx.update(|window, _| window.refresh());
        let editor = cx.read(|cx| {
            let editor = draft
                .read(cx)
                .authentication_editor
                .as_ref()
                .unwrap()
                .clone();
            let state = editor.read(cx);
            assert_eq!(state.username.read(cx).value(), "saved-user");
            assert_eq!(state.password.read(cx).value(), "saved-password");
            assert!(state.password.read(cx).presentation().is_masked());

            editor
        });
        let username = cx.debug_bounds("authentication-username").unwrap();
        cx.simulate_click(username.center(), Modifiers::default());
        cx.simulate_keystrokes("secondary-a");
        cx.simulate_input("edited-user");
        cx.read(|cx| {
            assert_eq!(
                draft.read(cx).request.authentication,
                Authentication::Basic {
                    username: "edited-user".into(),
                    password: "saved-password".into(),
                }
            );
        });

        // Changing the type must update the saved request immediately.
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.kind = AuthenticationKind::None;
                editor.changed(cx);
            })
        });
        cx.read(|cx| assert_eq!(draft.read(cx).request.authentication, Authentication::None));
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.kind = AuthenticationKind::Basic;
                editor.changed(cx);
            })
        });
        cx.read(|cx| {
            assert_eq!(
                draft.read(cx).request.authentication,
                Authentication::Basic {
                    username: "edited-user".into(),
                    password: "saved-password".into(),
                }
            )
        });
    }
}
