use environment::{GENERATED_VARIABLES, valid_variable_name};
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputEvent, InputState},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};

use super::{VariableScope, VariableStore};

pub(crate) fn open_manager(scope: Entity<VariableScope>, window: &mut Window, cx: &mut App) {
    let store = VariableStore::global(cx);
    let manager = cx.new(|cx| {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Variable name"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let mut subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        for input in [&name, &value] {
            subscriptions.push(cx.subscribe(input, |_, _, _: &InputEvent, cx| cx.notify()));
        }
        VariableManager {
            scope,
            store,
            name,
            value,
            secret: false,
            error: None,
            _subscriptions: subscriptions,
        }
    });
    let name = manager.read(cx).name.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        dialog
            .title("Variables")
            .w(rems(34.).to_pixels(window.rem_size()))
            .overlay_closable(false)
            .child(manager.clone())
    });
    name.update(cx, |input, cx| input.focus(window, cx));
}

struct VariableManager {
    scope: Entity<VariableScope>,
    store: Entity<VariableStore>,
    name: Entity<InputState>,
    value: Entity<InputState>,
    secret: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl VariableManager {
    fn select_source(&mut self, secret: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.secret = secret;
        self.error = None;
        self.name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.value.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_masked(secret, window, cx);
        });
        cx.notify();
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        if !valid_variable_name(&name) {
            self.error = Some(
                "Use letters, numbers, underscores, hyphens or dots in variable names.".into(),
            );
            cx.notify();
            return;
        }
        let value = self.value.read(cx).value().to_string();
        let scope = self.scope.read(cx).path.clone();
        self.store.update(cx, |store, cx| {
            store.save_entry(scope, self.secret, name, Some(value), cx)
        });
        self.error = None;
        cx.notify();
    }
}

impl Render for VariableManager {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let scope = self.scope.read(cx).path.clone();
        let environment_error = store.environment_errors.get(&scope);
        let unavailable = if self.secret {
            store.loading || store.secret_error.is_some()
        } else {
            environment_error.is_some()
        };
        let busy = store.saving;
        let mut names: Vec<_> = if self.secret {
            store.secrets.keys().cloned().collect()
        } else {
            store
                .environments
                .get(&scope)
                .map(|values| values.keys().cloned().collect())
                .unwrap_or_default()
        };
        names.sort();
        let error = self
            .error
            .clone()
            .or_else(|| store.save_error.clone())
            .or_else(|| {
                if self.secret {
                    store.secret_error.clone()
                } else {
                    environment_error.cloned()
                }
            });
        let loading = self.secret && store.loading;
        let can_retry = self.secret && store.secret_error.is_some();

        let source_tabs =
            h_flex()
                .gap_2()
                .children(
                    [(false, "Environment"), (true, "Secrets")].map(|(secret, label)| {
                        Button::new(label)
                            .debug_selector(move || format!("variable-source-{label}"))
                            .label(label)
                            .small()
                            .when(self.secret == secret, |button| button.primary())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_source(secret, window, cx)
                            }))
                    }),
                );
        let description = if self.secret {
            "App-wide secrets are stored in your OS keyring. Use {{vault:name}} in requests."
        } else if scope.is_some() {
            "This collection’s environment.toml. Use {{name}} in requests."
        } else {
            "Environment for unsaved requests. Use {{name}} in requests."
        };
        let list = v_flex()
            .id("variable-manager-list")
            .max_h(rems(10.))
            .overflow_y_scroll()
            .when(names.is_empty(), |list| {
                list.child(
                    div()
                        .p_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if loading {
                            "Loading secrets…"
                        } else {
                            "No variables yet"
                        }),
                )
            })
            .children(names.into_iter().enumerate().map(|(index, name)| {
                let selected_name = name.clone();
                let delete_name = name.clone();

                h_flex()
                    .gap_2()
                    .child(
                        Button::new(("edit-variable", index))
                            .debug_selector(move || format!("variable-manager-entry-{index}"))
                            .ghost()
                            .flex_1()
                            .child(div().w_full().text_left().child(name))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let value = if this.secret {
                                    this.store
                                        .read(cx)
                                        .secrets
                                        .get(&selected_name)
                                        .cloned()
                                        .unwrap_or_default()
                                } else {
                                    this.store
                                        .read(cx)
                                        .environments
                                        .get(&this.scope.read(cx).path)
                                        .and_then(|values| values.get(&selected_name))
                                        .cloned()
                                        .unwrap_or_default()
                                };

                                this.name.update(cx, |input, cx| {
                                    input.set_value(selected_name.clone(), window, cx)
                                });
                                this.value.update(cx, |input, cx| {
                                    input.set_value(value, window, cx);
                                    input.focus(window, cx);
                                });
                            })),
                    )
                    .when(self.secret, |row| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("••••••••"),
                        )
                    })
                    .child(
                        Button::new(("delete-variable", index))
                            .ghost()
                            .small()
                            .label("Remove")
                            .disabled(busy || unavailable)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let scope = this.scope.read(cx).path.clone();
                                this.store.update(cx, |store, cx| {
                                    store.save_entry(
                                        scope,
                                        this.secret,
                                        delete_name.clone(),
                                        None,
                                        cx,
                                    )
                                });
                            })),
                    )
            }));
        let form = h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.name).aria_label("Variable name")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.value).aria_label(if self.secret {
                        "Secret value"
                    } else {
                        "Variable value"
                    })),
            );
        let buttons = h_flex()
            .gap_2()
            .when(!self.secret, |row| {
                row.child(
                    Button::new("reload-environment")
                        .ghost()
                        .label("Reload file")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let scope = this.scope.read(cx).path.clone();
                            this.store
                                .update(cx, |store, cx| store.reload_environment(&scope, cx));
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("save-variable")
                    .primary()
                    .label(if busy { "Saving…" } else { "Save variable" })
                    .disabled(busy || unavailable)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .child(
                Button::new("close-variables")
                    .label("Done")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            );
        let generated = GENERATED_VARIABLES
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");

        v_flex()
            .debug_selector(|| "variable-manager".into())
            .gap_3()
            .child(source_tabs)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
            .child(list)
            .child(form)
            .when_some(error, |view, error| {
                view.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .when(can_retry, |view| {
                view.child(
                    Button::new("retry-secrets")
                        .label("Retry keyring")
                        .disabled(busy || loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.store.update(cx, |store, cx| store.load_secrets(cx))
                        })),
                )
            })
            .child(buttons)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Generated on Send: {generated}")),
            )
    }
}
