use environment::{Environment, EnvironmentLoadError, valid_variable_name};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{Environments, EnvironmentsEvent};

struct VariableRow {
    key: Entity<InputState>,
    value: Entity<InputState>,
    _subscriptions: [Subscription; 2],
}

/// Edits one global environment's name and variables in a tab. Variables are
/// written to the environment file only when saved.
pub struct EnvironmentEditor {
    pub name: SharedString,
    environments: Entity<Environments>,
    saved: Vec<(String, String)>,
    variables: Vec<(String, String)>,
    load_error: Option<String>,
    error: Option<String>,
    name_input: Option<Entity<InputState>>,
    rows: Vec<VariableRow>,
    _subscriptions: Vec<Subscription>,
}

impl EnvironmentEditor {
    pub fn new(name: SharedString, environments: Entity<Environments>, cx: &App) -> Self {
        let (saved, load_error) = match Environment::from_file(environments.read(cx).path(&name)) {
            Ok(environment) => (sorted(environment.entries), None),
            Err(EnvironmentLoadError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                (Vec::new(), None)
            }
            Err(error) => (Vec::new(), Some(error.to_string())),
        };

        Self {
            name,
            environments,
            variables: saved.clone(),
            saved,
            load_error,
            error: None,
            name_input: None,
            rows: Vec::new(),
            _subscriptions: Vec::new(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        sorted(self.variables.iter().cloned()) != self.saved
    }

    /// Select the name so a new or renamed environment can be typed over.
    pub fn focus_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare(window, cx);

        if let Some(input) = &self.name_input {
            input.update(cx, |input, cx| {
                input.select_all(window, cx);
                input.focus(window, cx);
            });
        }
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let result = self.write(cx);
        self.error = result.as_ref().err().cloned();
        cx.notify();

        result
    }

    fn write(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.commit_name(cx)?;

        if let Some(error) = &self.load_error {
            return Err(format!(
                "Fix the environment file before saving, so your changes do not replace it: {error}"
            ));
        }

        let mut entries = std::collections::HashMap::new();
        for (key, value) in &self.variables {
            if !valid_variable_name(key) {
                return Err(format!(
                    "\u{201c}{key}\u{201d} is not a valid variable name. Use letters, numbers, _, - or ."
                ));
            }

            if entries.insert(key.clone(), value.clone()).is_some() {
                return Err(format!(
                    "\u{201c}{key}\u{201d} is defined more than once. Remove or rename the duplicate."
                ));
            }
        }

        let path = self.environments.read(cx).path(&self.name);
        Environment { path, entries }
            .save_file()
            .map_err(|error| format!("Could not save environment: {error}"))?;

        self.saved = sorted(self.variables.iter().cloned());
        Ok(())
    }

    fn commit_name(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(input) = &self.name_input else {
            return Ok(());
        };

        let name = input.read(cx).value();
        if name.trim() == self.name.as_ref() {
            return Ok(());
        }

        let from = self.name.clone();
        let to = self
            .environments
            .update(cx, |environments, cx| environments.rename(&from, &name, cx))
            .map_err(|error| format!("Could not rename environment: {error}"))?;
        self.name = to;

        Ok(())
    }

    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_input.is_some() {
            return;
        }

        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Environment name")
                .default_value(self.name.clone())
        });
        let name_subscription = cx.subscribe_in(
            &name,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.error = this.commit_name(cx).err();

                    if this.error.is_some() {
                        let name = this.name.clone();
                        input.update(cx, |input, cx| input.set_value(name, window, cx));
                    }

                    cx.notify();
                }
                _ => {}
            },
        );

        // The editor that renamed an environment has already updated itself;
        // this keeps the visible name in step with the stored one.
        let rename_subscription = cx.subscribe_in(
            &self.environments,
            window,
            |this, _, event: &EnvironmentsEvent, window, cx| {
                if let EnvironmentsEvent::Renamed { to, .. } = event
                    && *to == this.name
                    && let Some(input) = &this.name_input
                    && input.read(cx).value() != *to
                {
                    input.update(cx, |input, cx| input.set_value(to.clone(), window, cx));
                }
            },
        );

        let active_subscription = cx.observe(&self.environments, |_, _, cx| cx.notify());

        self.name_input = Some(name);
        self._subscriptions = vec![name_subscription, rename_subscription, active_subscription];

        for (key, value) in self.variables.clone() {
            self.append_row(&key, &value, window, cx);
        }
        self.append_row("", "", window, cx);
    }

    fn append_row(&mut self, key: &str, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Add variable")
                .default_value(key.to_owned())
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value.to_owned())
        });
        let subscriptions = [&key, &value].map(|input| {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.rows.last().is_some_and(|row| populated(row, cx)) {
                        this.append_row("", "", window, cx);
                    }

                    this.sync(cx);
                }
            })
        });

        self.rows.push(VariableRow {
            key,
            value,
            _subscriptions: subscriptions,
        });
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        self.variables = self
            .rows
            .iter()
            .filter_map(|row| {
                let key = row.key.read(cx).value().trim().to_owned();

                (!key.is_empty()).then(|| (key, row.value.read(cx).value().to_string()))
            })
            .collect();
        self.error = None;

        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let active = self.environments.read(cx).active() == Some(&self.name);

        h_flex()
            .flex_none()
            .h_10()
            .gap_2()
            .child(
                Icon::new(IconName::Globe)
                    .size_4()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .debug_selector(|| "environment-name".into())
                    .w(rems(20.))
                    .min_w_0()
                    .when_some(self.name_input.as_ref(), |this, input| {
                        this.child(Input::new(input).small().aria_label("Environment name"))
                    }),
            )
            .child(div().flex_1())
            .child(if active {
                Button::new("deactivate-environment")
                    .debug_selector(|| "deactivate-environment".into())
                    .small()
                    .ghost()
                    .icon(IconName::CircleCheck)
                    .label("Active")
                    .tooltip("Stop using this environment")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.environments
                            .update(cx, |environments, cx| environments.set_active(None, cx));
                    }))
            } else {
                Button::new("activate-environment")
                    .debug_selector(|| "activate-environment".into())
                    .small()
                    .ghost()
                    .icon(IconName::Check)
                    .label("Set active")
                    .tooltip("Use this environment's variables in requests")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let name = this.name.clone();
                        this.environments.update(cx, |environments, cx| {
                            environments.set_active(Some(name), cx)
                        });
                    }))
            })
            .child(
                Button::new("save-environment")
                    .debug_selector(|| "save-environment".into())
                    .small()
                    .label("Save")
                    .disabled(!self.is_dirty())
                    .tooltip("Save variables")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.save(cx);
                    })),
            )
    }

    fn table(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .debug_selector(|| "environment-variables-table".into())
            .w_full()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_tokens().lg)
            .overflow_hidden()
            .child(
                h_flex()
                    .h_8()
                    .bg(cx.theme().table_head)
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .children(["Variable", "Value"].map(|label| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_2()
                            .when(label == "Variable", |cell| cell.border_r_1())
                            .border_color(cx.theme().border)
                            .flex()
                            .items_center()
                            .child(label)
                    })),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                let populated = populated(row, cx);

                h_flex()
                    .id(("environment-variable", row.key.entity_id()))
                    .h_8()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .hover(|row| row.bg(cx.theme().table_hover))
                    .child(
                        h_flex()
                            .debug_selector(move || format!("environment-key-{index}"))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&row.key)
                                        .small()
                                        .appearance(false)
                                        .aria_label(format!("Variable {}", index + 1)),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .debug_selector(move || format!("environment-value-{index}"))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&row.value)
                                        .small()
                                        .appearance(false)
                                        .aria_label(format!("Value {}", index + 1)),
                                ),
                            )
                            .when(populated, |cell| {
                                cell.child(
                                    Button::new("remove-variable")
                                        .debug_selector(move || {
                                            format!("environment-remove-{index}")
                                        })
                                        .mr_1()
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Close)
                                        .accessibility_label(format!(
                                            "Remove variable {}",
                                            index + 1
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.rows.remove(index);
                                            this.sync(cx);
                                        })),
                                )
                            }),
                    )
            }))
    }
}

fn populated(row: &VariableRow, cx: &App) -> bool {
    !row.key.read(cx).value().is_empty() || !row.value.read(cx).value().is_empty()
}

fn sorted(entries: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort();
    entries
}

impl Render for EnvironmentEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("environment-editor")
            .debug_selector(|| "environment-editor".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_4()
            .gap_2()
            .text_sm()
            .overflow_y_scrollbar()
            .child(self.header(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "While this environment is active, requests in every collection can use \
                         its variables as {{name}}. They take precedence over collection variables.",
                    ),
            )
            .when_some(
                self.error.clone().or_else(|| {
                    self.load_error
                        .clone()
                        .map(|error| format!("Could not read this environment: {error}"))
                }),
                |this, error| {
                    this.child(
                        div()
                            .debug_selector(|| "environment-error".into())
                            .text_color(cx.theme().danger)
                            .child(error),
                    )
                },
            )
            .child(self.table(cx))
    }
}

impl crate::TabPage for EnvironmentEditor {
    fn tab_state(&self) -> crate::TabState {
        crate::TabState {
            badge: None,
            icon: Some("icons/globe.svg"),
            dirty: self.is_dirty(),
        }
    }

    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        EnvironmentEditor::prepare(self, window, cx);
    }
}
