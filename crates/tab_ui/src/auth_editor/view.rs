use gpui_kit::component::{
    button::*,
    checkbox::Checkbox,
    input::{Editor, Input},
    select::Select,
    switch::Switch,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Auth, AuthKind};

use super::editor::{AuthEditor, AuthTarget};
use super::fields::{Choice, Field, Row, description, rows};
use crate::variable_input::{VariableTarget, with_variables};

impl AuthEditor {
    /// A field's label beside its control.
    fn row(label: &'static str, top: bool, control: impl IntoElement, cx: &App) -> Div {
        h_flex()
            .w_full()
            .gap_3()
            .when(top, |row| row.items_start())
            .when(!top, |row| row.items_center())
            .child(
                div()
                    .w(rems(9.))
                    .flex_none()
                    .when(top, |label| label.pt_1())
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(control))
    }

    fn text_row(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let input = self.input(field, window, cx);
        let control = match (&input.target, &input.completion) {
            (VariableTarget::Input(input), Some(completion)) => {
                with_variables(completion, Input::new(input).aria_label(field.label()))
            }
            (VariableTarget::Input(input), None) => {
                div().child(Input::new(input).aria_label(field.label()).mask_toggle())
            }
            (VariableTarget::Editor(editor), completion) => {
                let editor = Editor::new(editor)
                    .h(rems(8.))
                    .text_sm()
                    .aria_label(field.label());
                match completion {
                    Some(completion) => with_variables(completion, editor),
                    None => div().child(editor),
                }
            }
        };
        let label = field.label();

        Self::row(
            label,
            field.multiline(),
            div()
                .debug_selector(move || format!("auth-field-{label}"))
                .child(control),
            cx,
        )
    }

    fn choice_row(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let select = self.choice(choice, window, cx);
        let label = choice.label();

        Self::row(
            label,
            false,
            div()
                .debug_selector(move || format!("auth-choice-{label}"))
                .max_w(rems(20.))
                .child(Select::new(&select).accessibility_label(label).w_full()),
            cx,
        )
    }

    fn token_row(&self, cx: &mut Context<Self>) -> Div {
        let waiting = self.token_task.is_some();
        let signing_in = matches!(&self.auth, Auth::OAuth2(auth)
            if auth.grant_type == request::OAuth2Grant::AuthorizationCode);

        h_flex()
            .gap_2()
            .pt_1()
            .child(
                Button::new("auth-get-token")
                    .debug_selector(|| "auth-get-token".into())
                    .primary()
                    .label(match waiting {
                        true if signing_in => "Waiting for sign-in…",
                        true => "Requesting…",
                        false => "Get New Access Token",
                    })
                    .disabled(waiting)
                    .on_click(cx.listener(|this, _, window, cx| this.get_token(window, cx))),
            )
            .when(waiting, |row| {
                row.child(
                    Button::new("auth-cancel-token")
                        .debug_selector(|| "auth-cancel-token".into())
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_token(cx))),
                )
            })
            .when(waiting && signing_in, |row| {
                row.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Sign in with your browser to continue."),
                )
            })
    }

    /// What an inheriting request sends.
    fn inherited_note(&self, cx: &App) -> Div {
        let muted = cx.theme().muted_foreground;
        let note = div().debug_selector(|| "auth-inherited".into());

        let Some(inherited) = &self.inherited else {
            return note.text_color(muted).child(
                "This request is not saved in a collection, so it sends no authorization. Save it in a collection to use the collection's.",
            );
        };

        if inherited.auth.is_unset() {
            return note.text_color(muted).child(format!(
                "{} has no authorization. Set one in the collection's Auth tab.",
                inherited.name
            ));
        }

        let kind = inherited.auth.kind();
        note.child(
            v_flex()
                .gap_2()
                .child(format!(
                    "This request uses {} from {}.",
                    kind.label(),
                    inherited.name
                ))
                .when(
                    self.target == AuthTarget::Grpc && !kind.supports_grpc(),
                    |note| {
                        note.child(div().text_color(cx.theme().danger).child(format!(
                            "{} cannot authorize gRPC calls. Choose another authorization for this request.",
                            kind.label()
                        )))
                    },
                ),
        )
    }
}

impl Render for AuthEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kinds = self.kinds(window, cx);
        let kind = self.auth.kind();
        let mut content = Vec::new();

        for row in rows(&self.auth, self.target.has_query()) {
            content.push(match row {
                Row::Text(field) => self.text_row(field, window, cx),
                Row::Choice(choice) => self.choice_row(choice, window, cx),
                Row::Pkce => {
                    let pkce = matches!(&self.auth, Auth::OAuth2(auth) if auth.pkce);
                    Self::row(
                        "PKCE",
                        false,
                        Switch::new("auth-pkce")
                            .accessibility_label("Use PKCE")
                            .checked(pkce)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Auth::OAuth2(auth) = &mut this.auth {
                                    auth.pkce = *checked;
                                    this.changed(cx);
                                }
                            })),
                        cx,
                    )
                }
                Row::SecretBase64 => {
                    let base64 = matches!(&self.auth, Auth::Jwt(auth) if auth.secret_base64);
                    Self::row(
                        "",
                        false,
                        Checkbox::new("auth-secret-base64")
                            .small()
                            .label("Secret is Base64 encoded")
                            .checked(base64)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Auth::Jwt(auth) = &mut this.auth {
                                    auth.secret_base64 = *checked;
                                    this.changed(cx);
                                }
                            })),
                        cx,
                    )
                }
                // A line separates a group from the rows above it.
                Row::Heading(heading) => div()
                    .w_full()
                    .when(!content.is_empty(), |heading| {
                        heading.pt_3().border_t_1().border_color(cx.theme().border)
                    })
                    .font_weight(FontWeight::MEDIUM)
                    .child(heading),
                Row::GetToken => self.token_row(cx),
            });
        }

        if kind == AuthKind::Inherit {
            content.push(self.inherited_note(cx));
        }

        h_flex()
            .debug_selector(|| "auth-editor".into())
            .w_full()
            .items_start()
            .flex_wrap()
            .gap_6()
            .pb_3()
            .child(
                v_flex()
                    .w(rems(16.))
                    .flex_none()
                    .gap_2()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Auth Type"))
                    .child(
                        div().debug_selector(|| "auth-type".into()).child(
                            Select::new(&kinds)
                                .accessibility_label("Auth type")
                                .w_full(),
                        ),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(description(kind, self.target == AuthTarget::Grpc)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(rems(20.))
                    .max_w(rems(40.))
                    .gap_3()
                    .children(content),
            )
    }
}
