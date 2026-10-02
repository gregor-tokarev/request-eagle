use flow::{BlockKind, DisplayFormat, Field, Flow, TemplateFormat};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;
use serde_json::Value;

use super::{FlowEditor, blocks, preview};

/// A setting of a block that an editor of the inspector changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Setting {
    Title,
    StartInput,
    Expression,
    Condition(usize),
    Variable(usize),
    Schema,
    Milliseconds,
    Text,
    Number,
    Date,
    Path,
    FieldKey(usize),
    FieldValue(usize),
    Item(usize),
    Template,
    Name,
    Output(usize),
    Note,
}

/// Lists of a block's settings that the inspector adds to and removes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum List {
    Variables,
    Conditions,
    Fields,
    Items,
    Outputs,
}

enum Editing {
    Line(Entity<InputState>),
    Code(Entity<EditorState>),
}

impl Editing {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Line(input) => input.focus_handle(cx),
            Self::Code(editor) => editor.focus_handle(cx),
        }
    }
}

/// The settings and last run of the selected block.
pub(super) struct Inspector {
    pub block: String,
    editors: Vec<(Setting, Editing)>,
    request_search: Option<Entity<InputState>>,
    /// The result of the block's FQL with the inputs of its last run.
    preview: Option<Result<String, String>>,
    /// The last run's inputs and outputs.
    run: Option<Entity<EditorState>>,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.editors
            .iter()
            .any(|(_, editor)| editor.focus_handle(cx).contains_focused(window, cx))
            || self
                .request_search
                .as_ref()
                .is_some_and(|search| search.focus_handle(cx).is_focused(window))
    }

    fn editor(&self, setting: Setting) -> Option<&Editing> {
        self.editors
            .iter()
            .find(|(candidate, _)| *candidate == setting)
            .map(|(_, editor)| editor)
    }
}

impl FlowEditor {
    /// Show the settings of the selected block, when one block is selected.
    pub(super) fn sync_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = match self.selection.as_slice() {
            [id] => Some(id.clone()),
            _ => None,
        };

        match selected {
            Some(id)
                if self
                    .inspector
                    .as_ref()
                    .is_some_and(|inspector| inspector.block == id) => {}
            Some(id) => self.inspector = Some(self.build_inspector(&id, window, cx)),
            None => self.inspector = None,
        }
    }

    /// Show the latest run in the inspector.
    pub(super) fn refresh_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .inspector
            .as_ref()
            .map(|inspector| inspector.block.clone())
        else {
            return;
        };

        let run = self.run_view(&id, window, cx);
        let preview = self.preview(&id);
        if let Some(inspector) = &mut self.inspector {
            inspector.run = run;
            inspector.preview = preview;
        }
    }

    /// Edit the selected block's title.
    pub(super) fn focus_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Editing::Line(title)) = self
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.editor(Setting::Title))
        {
            title.update(cx, |title, cx| {
                title.select_all(window, cx);
                title.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn build_inspector(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Inspector {
        let mut inspector = Inspector {
            block: id.to_owned(),
            editors: Vec::new(),
            request_search: None,
            preview: None,
            run: None,
            _subscriptions: Vec::new(),
        };
        let Some(block) = self.flow.block(id).cloned() else {
            return inspector;
        };

        let mut line = |setting: Setting,
                        value: &str,
                        placeholder: &str,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder.to_owned())
                    .default_value(value.to_owned())
            });
            inspector
                ._subscriptions
                .push(subscribe_line(&input, id, setting, cx));
            inspector.editors.push((setting, Editing::Line(input)));
        };

        line(
            Setting::Title,
            block.title.as_deref().unwrap_or_default(),
            block.kind.block_type().name(),
            window,
            cx,
        );

        if let Some(variables) = block.kind.variables() {
            for (index, variable) in variables.iter().enumerate() {
                line(
                    Setting::Variable(index),
                    variable,
                    "Variable name",
                    window,
                    cx,
                );
            }
        }

        match &block.kind {
            BlockKind::Delay { milliseconds } => line(
                Setting::Milliseconds,
                &milliseconds.to_string(),
                "Milliseconds",
                window,
                cx,
            ),
            BlockKind::Number { value } => {
                line(Setting::Number, &value.to_string(), "0", window, cx)
            }
            BlockKind::Date { value } => {
                line(Setting::Date, value, "2024-05-01T09:30:00Z", window, cx)
            }
            BlockKind::Select { path } => line(Setting::Path, path, "body.items.0.id", window, cx),
            BlockKind::SetVariable { name } | BlockKind::GetVariable { name } => {
                line(Setting::Name, name, "Variable name", window, cx)
            }
            BlockKind::Record { fields } => {
                for (index, field) in fields.iter().enumerate() {
                    line(Setting::FieldKey(index), &field.key, "Key", window, cx);
                    line(
                        Setting::FieldValue(index),
                        &field.value,
                        "Value when not connected",
                        window,
                        cx,
                    );
                }
            }
            BlockKind::List { items } => {
                for (index, item) in items.iter().enumerate() {
                    line(
                        Setting::Item(index),
                        item,
                        "Value when not connected",
                        window,
                        cx,
                    );
                }
            }
            BlockKind::Output { names } => {
                for (index, name) in names.iter().enumerate() {
                    line(Setting::Output(index), name, "Output name", window, cx);
                }
            }
            _ => {}
        }

        let mut code = |setting: Setting,
                        value: &str,
                        language: &str,
                        placeholder: &str,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            let editor = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(language.to_owned())
                    .line_number(false)
                    .soft_wrap(true)
                    .placeholder(placeholder.to_owned())
                    .default_value(value.to_owned())
            });
            inspector
                ._subscriptions
                .push(subscribe_code(&editor, id, setting, cx));
            inspector.editors.push((setting, Editing::Code(editor)));
        };

        match &block.kind {
            BlockKind::Start { input } => code(
                Setting::StartInput,
                input,
                "json",
                "{\"key\": \"value\"}",
                window,
                cx,
            ),
            BlockKind::Evaluate { expression, .. } => code(
                Setting::Expression,
                expression,
                "text",
                "value1.body.items[status = 'active'].id",
                window,
                cx,
            ),
            BlockKind::If { condition, .. } => code(
                Setting::Condition(0),
                condition,
                "text",
                "value1.http.status = 200",
                window,
                cx,
            ),
            BlockKind::Condition { conditions, .. } => {
                for (index, condition) in conditions.iter().enumerate() {
                    code(
                        Setting::Condition(index),
                        condition,
                        "text",
                        "value1 > 10",
                        window,
                        cx,
                    );
                }
            }
            BlockKind::Validate { schema } => code(
                Setting::Schema,
                schema,
                "json",
                "{\"type\": \"object\"}",
                window,
                cx,
            ),
            BlockKind::String { value } => code(Setting::Text, value, "text", "Text", window, cx),
            BlockKind::Template { template, .. } => code(
                Setting::Template,
                template,
                "text",
                "Hello {{value1}}",
                window,
                cx,
            ),
            BlockKind::Note { text } => {
                code(Setting::Note, text, "text", "Write a note", window, cx)
            }
            _ => {}
        }

        if matches!(block.kind, BlockKind::HttpRequest { .. }) {
            let search =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search saved requests"));
            inspector
                ._subscriptions
                .push(cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }));
            inspector.request_search = Some(search);
        }

        inspector.preview = self.preview(id);
        inspector.run = self.run_view(id, window, cx);
        inspector
    }

    /// A read-only view of the block's last run.
    fn run_view(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<EditorState>> {
        let run = self.run.blocks.get(id)?.last.as_ref()?;
        let object = |values: &[(String, std::sync::Arc<Value>)]| {
            Value::Object(
                values
                    .iter()
                    .map(|(name, value)| (name.clone(), (**value).clone()))
                    .collect(),
            )
        };
        // Large responses would make the view slow to lay out.
        let text = preview::json(
            &serde_json::json!({
                "inputs": object(&run.inputs),
                "outputs": object(&run.outputs),
            }),
            true,
            256 * 1024,
        );

        Some(cx.new(|cx| {
            let mut editor = EditorState::new(window, cx)
                .language("json")
                .line_number(false)
                .soft_wrap(true)
                .default_value(text);
            editor.set_readonly(true, cx);
            editor
        }))
    }

    /// Evaluate the block's FQL with the inputs of its last run, or check
    /// that it parses when it has not run.
    fn preview(&self, id: &str) -> Option<Result<String, String>> {
        let block = self.flow.block(id)?;
        let source = match &block.kind {
            BlockKind::Evaluate { expression, .. } => expression,
            BlockKind::If { condition, .. } => condition,
            _ => return None,
        };
        if source.trim().is_empty() {
            return None;
        }

        let expression = match fql::Expression::parse(source) {
            Ok(expression) => expression,
            Err(error) => return Some(Err(error.to_string())),
        };
        let run = self
            .run
            .blocks
            .get(id)
            .and_then(|status| status.last.as_ref())?;
        let input = Value::Object(
            run.inputs
                .iter()
                .filter(|(name, _)| {
                    block
                        .kind
                        .variables()
                        .is_some_and(|variables| variables.contains(name))
                })
                .map(|(name, value)| (name.clone(), (**value).clone()))
                .collect(),
        );

        Some(
            match expression.evaluate(Some(&input), &fql::Bindings::default()) {
                Ok(Some(value)) => Ok(preview::pretty(&value)),
                Ok(None) => Ok("undefined".to_owned()),
                Err(error) => Err(error.to_string()),
            },
        )
    }

    /// Change one setting of a block from its editor.
    pub(super) fn apply_setting(
        &mut self,
        id: &str,
        setting: Setting,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let key = Some(format!("{id}:{setting:?}"));
        let id = id.to_owned();

        self.edit(key, |flow| apply(flow, &id, setting, value), cx);

        let preview = self.preview(&id);
        if let Some(inspector) = self
            .inspector
            .as_mut()
            .filter(|inspector| inspector.block == id)
        {
            inspector.preview = preview;
        }
    }

    pub(super) fn set_kind(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut BlockKind),
        cx: &mut Context<Self>,
    ) {
        let id = id.to_owned();
        self.request_info.clear();
        self.edit(
            None,
            |flow| {
                if let Some(block) = flow.block_mut(&id) {
                    change(&mut block.kind);
                }
            },
            cx,
        );
    }

    /// Add an entry to one of a block's lists, or remove one, keeping the
    /// connections of the ports that remain.
    fn change_list(
        &mut self,
        id: &str,
        list: List,
        remove: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = id.to_owned();
        self.edit(None, |flow| change_list(flow, &id, list, remove), cx);
        self.inspector = None;
        self.sync_inspector(window, cx);
    }

    pub(super) fn render_inspector(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let panel = v_flex()
            .id("flow-inspector")
            .debug_selector(|| "flow-inspector".into())
            .flex_none()
            .w(rems(22.))
            .h_full()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.background)
            .overflow_y_scroll()
            .p_3()
            .gap_3();

        if let Some(connection) = &self.selected_connection {
            let title = |id: &str| {
                self.flow
                    .block(id)
                    .map(|block| block.title().to_owned())
                    .unwrap_or_default()
            };
            let connection = connection.clone();
            return Some(
                panel
                    .child(section_title("Connection"))
                    .child(div().text_sm().child(format!(
                        "{} · {} → {} · {}",
                        title(&connection.from),
                        blocks::port_label(&connection.output.clone().into()),
                        title(&connection.to),
                        blocks::port_label(&connection.input.clone().into()),
                    )))
                    .child(
                        Button::new("flow-delete-connection")
                            .small()
                            .danger()
                            .label("Delete connection")
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.delete_selection(window, cx)
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if self.selection.len() > 1 {
            return Some(
                panel
                    .child(section_title(&format!("{} blocks", self.selection.len())))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("flow-duplicate-selection")
                                    .small()
                                    .label("Duplicate")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.duplicate(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("flow-delete-selection")
                                    .small()
                                    .danger()
                                    .label("Delete")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.delete_selection(window, cx)
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }

        let inspector = self.inspector.as_ref()?;
        let block = self.flow.block(&inspector.block)?;
        let block_type = block.kind.block_type();
        let id = block.id.clone();

        let field = |setting: Setting| -> Option<AnyElement> {
            match inspector.editor(setting)? {
                Editing::Line(input) => Some(Input::new(input).small().into_any_element()),
                Editing::Code(editor) => Some(
                    div()
                        .h(rems(8.))
                        .rounded(theme.radius_tokens().md)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(Editor::new(editor).h_full().bordered(false).text_sm())
                        .into_any_element(),
                ),
            }
        };

        let mut panel =
            panel
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Icon::default()
                                .path(blocks::icon(block_type))
                                .size_4()
                                .text_color(blocks::color(block_type, cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(block_type.name()),
                        )
                        .child(
                            Button::new("flow-delete-block")
                                .debug_selector(|| "flow-delete-block".into())
                                .xsmall()
                                .ghost()
                                .icon(Icon::default().path("icons/trash.svg"))
                                .accessibility_label("Delete block")
                                .tooltip("Delete block")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.delete_selection(window, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(block_type.description()),
                )
                .child(labeled("Title", field(Setting::Title)));

        if let Some(variables) = block.kind.variables() {
            let rows = (0..variables.len()).map(|index| {
                list_row(
                    field(Setting::Variable(index)),
                    remove_button(&id, List::Variables, index, cx),
                )
            });
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(section_title("Variables"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Each is an input; FQL reads it by name, such as value1.body."),
                    )
                    .children(rows)
                    .child(add_button(&id, List::Variables, "Add variable", cx)),
            );
        }

        panel = match &block.kind {
            BlockKind::Start { .. } => panel.child(labeled_note(
                "Input",
                field(Setting::StartInput),
                "JSON the Start block sends when you run the flow here. The CLI can send other input.",
                cx,
            )),
            BlockKind::HttpRequest { request } => panel.child(self.request_chooser(&id, request, inspector, cx)),
            BlockKind::Evaluate { .. } => panel
                .child(labeled_note("Expression", field(Setting::Expression), "FQL, the JSONata-based language of Postman Flows.", cx))
                .child(self.preview_element(inspector, cx)),
            BlockKind::If { .. } => panel
                .child(labeled_note("Condition", field(Setting::Condition(0)), "Data goes out of Then when this is true, of Else otherwise.", cx))
                .child(self.preview_element(inspector, cx)),
            BlockKind::Condition { conditions, .. } => panel.child(
                v_flex()
                    .gap_1()
                    .child(section_title("Conditions"))
                    .children((0..conditions.len()).map(|index| {
                        v_flex()
                            .gap_1()
                            .child(list_row(
                                Some(div().text_xs().child(format!("Condition {}", index + 1)).into_any_element()),
                                remove_button(&id, List::Conditions, index, cx),
                            ))
                            .children(field(Setting::Condition(index)))
                    }))
                    .child(add_button(&id, List::Conditions, "Add condition", cx)),
            ),
            BlockKind::Validate { .. } => panel.child(labeled("JSON Schema", field(Setting::Schema))),
            BlockKind::Delay { .. } => panel.child(labeled("Milliseconds", field(Setting::Milliseconds))),
            BlockKind::Display { format } => panel.child(labeled(
                "Format",
                Some(
                    h_flex()
                        .gap_1()
                        .children(DisplayFormat::ALL.into_iter().map(|option| {
                            let id = id.clone();
                            Button::new(SharedString::from(format!("display-format-{}", option.label())))
                                .xsmall()
                                .selected(*format == option)
                                .label(option.label())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_kind(&id, |kind| {
                                        if let BlockKind::Display { format } = kind {
                                            *format = option;
                                        }
                                    }, cx);
                                }))
                        }))
                        .into_any_element(),
                ),
            )),
            BlockKind::String { .. } => panel.child(labeled("Value", field(Setting::Text))),
            BlockKind::Number { .. } => panel.child(labeled("Value", field(Setting::Number))),
            BlockKind::Boolean { value } => {
                let id = id.clone();
                let checked = *value;
                panel.child(
                    Switch::new("flow-boolean")
                        .checked(checked)
                        .label(if checked { "True" } else { "False" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_kind(&id, |kind| {
                                if let BlockKind::Boolean { value } = kind {
                                    *value = !*value;
                                }
                            }, cx);
                        })),
                )
            }
            BlockKind::Date { .. } => panel.child(labeled("ISO 8601 date", field(Setting::Date))),
            BlockKind::Select { .. } => panel.child(labeled_note(
                "Path",
                field(Setting::Path),
                "Dotted keys and list indexes, such as body.items.0.id. Empty selects everything.",
                cx,
            )),
            BlockKind::Record { fields } => panel.child(
                v_flex()
                    .gap_1()
                    .child(section_title("Fields"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Each key is an input. Without a connection it holds its value, read as JSON when it is JSON."),
                    )
                    .children((0..fields.len()).map(|index| {
                        list_row(
                            Some(
                                h_flex()
                                    .gap_1()
                                    .children(field(Setting::FieldKey(index)))
                                    .children(field(Setting::FieldValue(index)))
                                    .into_any_element(),
                            ),
                            remove_button(&id, List::Fields, index, cx),
                        )
                    }))
                    .child(add_button(&id, List::Fields, "Add field", cx)),
            ),
            BlockKind::List { items } => panel.child(
                v_flex()
                    .gap_1()
                    .child(section_title("Items"))
                    .children((0..items.len()).map(|index| {
                        list_row(field(Setting::Item(index)), remove_button(&id, List::Items, index, cx))
                    }))
                    .child(add_button(&id, List::Items, "Add item", cx)),
            ),
            BlockKind::Template { format, .. } => {
                let format = *format;
                panel
                    .child(labeled_note(
                        "Template",
                        field(Setting::Template),
                        "{{name}} fills in a variable; {{#list}}…{{/list}} repeats for each item.",
                        cx,
                    ))
                    .child(labeled(
                        "Sends",
                        Some(
                            h_flex()
                                .gap_1()
                                .children([(TemplateFormat::Text, "Text"), (TemplateFormat::Json, "JSON")].map(|(option, label)| {
                                    let id = id.clone();
                                    Button::new(SharedString::from(format!("template-format-{label}")))
                                        .xsmall()
                                        .selected(format == option)
                                        .label(label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_kind(&id, |kind| {
                                                if let BlockKind::Template { format, .. } = kind {
                                                    *format = option;
                                                }
                                            }, cx);
                                        }))
                                }))
                                .into_any_element(),
                        ),
                    ))
            }
            BlockKind::SetVariable { .. } | BlockKind::GetVariable { .. } => {
                panel.child(labeled("Name", field(Setting::Name)))
            }
            BlockKind::Output { names } => panel.child(
                v_flex()
                    .gap_1()
                    .child(section_title("Outputs"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("What runs return under each name, such as from the CLI."),
                    )
                    .children((0..names.len()).map(|index| {
                        list_row(field(Setting::Output(index)), remove_button(&id, List::Outputs, index, cx))
                    }))
                    .child(add_button(&id, List::Outputs, "Add output", cx)),
            ),
            BlockKind::Note { .. } => panel.child(labeled("Text", field(Setting::Note))),
            BlockKind::Or
            | BlockKind::Repeat
            | BlockKind::For
            | BlockKind::Collect
            | BlockKind::Log
            | BlockKind::Null
            | BlockKind::Now => panel,
        };

        Some(
            panel
                .when_some(inspector.run.as_ref(), |this, run| {
                    this.child(
                        v_flex().gap_1().child(section_title("Last run")).child(
                            div()
                                .h(rems(16.))
                                .rounded(theme.radius_tokens().md)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(
                                    Editor::new(run)
                                        .h_full()
                                        .bordered(false)
                                        .readonly(true)
                                        .text_xs(),
                                ),
                        ),
                    )
                })
                .into_any_element(),
        )
    }

    fn preview_element(&self, inspector: &Inspector, cx: &App) -> AnyElement {
        let theme = cx.theme();

        match &inspector.preview {
            None => div().into_any_element(),
            Some(Ok(result)) => v_flex()
                .gap_1()
                .child(section_title("Result with the last inputs"))
                .child(
                    div()
                        .debug_selector(|| "flow-fql-preview".into())
                        .p_2()
                        .rounded(theme.radius_tokens().md)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .child(result.clone()),
                )
                .into_any_element(),
            Some(Err(error)) => div()
                .debug_selector(|| "flow-fql-error".into())
                .text_xs()
                .text_color(theme.danger)
                .child(error.clone())
                .into_any_element(),
        }
    }

    fn request_chooser(
        &self,
        id: &str,
        current: &str,
        inspector: &Inspector,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let chosen = (!current.is_empty())
            .then(|| self.requests.find(current, cx))
            .flatten();
        let query = inspector
            .request_search
            .as_ref()
            .map(|search| search.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let matches: Vec<_> = self
            .requests
            .all(cx)
            .into_iter()
            .filter(|request| {
                query.is_empty()
                    || request.name.to_lowercase().contains(&query)
                    || request.request.path.to_lowercase().contains(&query)
                    || request.location.to_lowercase().contains(&query)
            })
            .take(30)
            .collect();

        v_flex()
            .gap_2()
            .child(section_title("Request"))
            .child(match &chosen {
                Some(request) => v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(theme.radius_tokens().md)
                    .bg(theme.muted)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(method_label(request.request.method.as_str(), cx))
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(request.name.clone())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .text_ellipsis()
                            .child(SharedString::from(request.request.path.clone())),
                    )
                    .child(
                        div().text_xs().text_color(theme.muted_foreground).child(
                            match flow::request_variables(&request.request) {
                                variables if variables.is_empty() => "No variables to fill".to_owned(),
                                variables => format!("Inputs: {}", variables.join(", ")),
                            },
                        ),
                    )
                    .into_any_element(),
                None => div()
                    .text_sm()
                    .text_color(if current.is_empty() { theme.muted_foreground } else { theme.danger })
                    .child(if current.is_empty() {
                        "Choose the saved request this block sends."
                    } else {
                        "The chosen request is no longer saved."
                    })
                    .into_any_element(),
            })
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Success sends 2xx responses, Fail the others. Each {{variable}} is an input; a connected value replaces it."),
            )
            .children(inspector.request_search.as_ref().map(|search| {
                Input::new(search).small().prefix(IconName::Search)
            }))
            .child(
                v_flex()
                    .gap_px()
                    .when(matches.is_empty(), |this| {
                        this.child(div().text_xs().text_color(theme.muted_foreground).child("No saved HTTP requests match"))
                    })
                    .children(matches.into_iter().enumerate().map(|(index, request)| {
                        let block = id.to_owned();
                        let request_id = request.id.clone();
                        let selected = request.id == current;
                        h_flex()
                            .id(("flow-request-option", index))
                            .debug_selector(move || format!("flow-request-option-{index}"))
                            .h_8()
                            .px_2()
                            .gap_2()
                            .rounded(theme.radius_tokens().md)
                            .text_sm()
                            .when(selected, |this| this.bg(theme.accent))
                            .hover(|this| this.bg(theme.accent.opacity(0.7)))
                            .child(div().flex_none().child(method_label(request.request.method.as_str(), cx)))
                            .child(div().flex_none().child(request.name.clone()))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_ellipsis()
                                    .text_color(theme.muted_foreground)
                                    .child(request.location.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let request_id = request_id.clone();
                                this.set_kind(&block, |kind| {
                                    if let BlockKind::HttpRequest { request } = kind {
                                        *request = request_id;
                                    }
                                }, cx);
                            }))
                    })),
            )
            .into_any_element()
    }
}

fn subscribe_line(
    input: &Entity<InputState>,
    id: &str,
    setting: Setting,
    cx: &mut Context<FlowEditor>,
) -> Subscription {
    let id = id.to_owned();
    cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            let value = input.read(cx).value().to_string();
            this.apply_setting(&id, setting, value, cx);
        }
    })
}

fn subscribe_code(
    editor: &Entity<EditorState>,
    id: &str,
    setting: Setting,
    cx: &mut Context<FlowEditor>,
) -> Subscription {
    let id = id.to_owned();
    cx.subscribe(editor, move |this, editor, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            let value = editor.read(cx).value().to_string();
            this.apply_setting(&id, setting, value, cx);
        }
    })
}

/// Set a block's setting to text typed into its editor. Renaming a port
/// keeps its connections.
pub(super) fn apply(flow: &mut Flow, id: &str, setting: Setting, value: String) {
    let mut renamed = None;
    {
        let Some(block) = flow.block_mut(id) else {
            return;
        };

        match (setting, &mut block.kind) {
            (Setting::Title, _) => block.title = (!value.trim().is_empty()).then_some(value),
            (Setting::StartInput, BlockKind::Start { input }) => *input = value,
            (Setting::Expression, BlockKind::Evaluate { expression, .. }) => *expression = value,
            (Setting::Condition(_), BlockKind::If { condition, .. }) => *condition = value,
            (Setting::Condition(index), BlockKind::Condition { conditions, .. }) => {
                if let Some(condition) = conditions.get_mut(index) {
                    *condition = value;
                }
            }
            (Setting::Variable(index), kind) => {
                if let Some(variable) = kind
                    .variables_mut()
                    .and_then(|variables| variables.get_mut(index))
                {
                    renamed = Some((
                        false,
                        std::mem::replace(variable, value.trim().to_owned()),
                        value.trim().to_owned(),
                    ));
                }
            }
            (Setting::Schema, BlockKind::Validate { schema }) => *schema = value,
            (Setting::Milliseconds, BlockKind::Delay { milliseconds }) => {
                if let Ok(parsed) = value.trim().parse() {
                    *milliseconds = parsed;
                }
            }
            (Setting::Text, BlockKind::String { value: text }) => *text = value,
            (Setting::Number, BlockKind::Number { value: number }) => {
                if let Ok(parsed) = value.trim().parse::<f64>()
                    && parsed.is_finite()
                {
                    *number = parsed;
                }
            }
            (Setting::Date, BlockKind::Date { value: date }) => *date = value,
            (Setting::Path, BlockKind::Select { path }) => *path = value,
            (Setting::FieldKey(index), BlockKind::Record { fields }) => {
                if let Some(field) = fields.get_mut(index) {
                    renamed = Some((
                        false,
                        std::mem::replace(&mut field.key, value.clone()),
                        value,
                    ));
                }
            }
            (Setting::FieldValue(index), BlockKind::Record { fields }) => {
                if let Some(field) = fields.get_mut(index) {
                    field.value = value;
                }
            }
            (Setting::Item(index), BlockKind::List { items }) => {
                if let Some(item) = items.get_mut(index) {
                    *item = value;
                }
            }
            (Setting::Template, BlockKind::Template { template, .. }) => *template = value,
            (Setting::Name, BlockKind::SetVariable { name } | BlockKind::GetVariable { name }) => {
                *name = value
            }
            (Setting::Output(index), BlockKind::Output { names }) => {
                if let Some(name) = names.get_mut(index) {
                    renamed = Some((false, std::mem::replace(name, value.clone()), value));
                }
            }
            (Setting::Note, BlockKind::Note { text }) => *text = value,
            _ => {}
        }
    }

    if let Some((output, from, to)) = renamed {
        flow.rename_port(id, output, &from, &to);
    }
}

/// Add to a block's list, or remove its `remove`th entry. Ports numbered
/// after a removed one move up, and so do their connections.
pub(super) fn change_list(flow: &mut Flow, id: &str, list: List, remove: Option<usize>) {
    let Some(block) = flow.block(id) else {
        return;
    };
    // The port that goes with an entry: its name and whether it is an output.
    let port = |kind: &BlockKind, index: usize| -> Option<(bool, String)> {
        match (list, kind) {
            (List::Variables, kind) => kind
                .variables()?
                .get(index)
                .map(|name| (false, name.clone())),
            (List::Conditions, BlockKind::Condition { .. }) => {
                Some((true, format!("condition{}", index + 1)))
            }
            (List::Fields, BlockKind::Record { fields }) => {
                fields.get(index).map(|field| (false, field.key.clone()))
            }
            (List::Items, BlockKind::List { .. }) => Some((false, format!("item{}", index + 1))),
            (List::Outputs, BlockKind::Output { names }) => {
                names.get(index).map(|name| (false, name.clone()))
            }
            _ => None,
        }
    };
    // Conditions and items have numbered ports, which follow their place.
    let numbered = match list {
        List::Conditions => Some("condition"),
        List::Items => Some("item"),
        _ => None,
    };
    let length = match (list, &block.kind) {
        (List::Variables, kind) => kind.variables().map_or(0, Vec::len),
        (List::Conditions, BlockKind::Condition { conditions, .. }) => conditions.len(),
        (List::Fields, BlockKind::Record { fields }) => fields.len(),
        (List::Items, BlockKind::List { items }) => items.len(),
        (List::Outputs, BlockKind::Output { names }) => names.len(),
        _ => 0,
    };

    if let Some(index) = remove {
        let Some((output, name)) = port(&block.kind, index) else {
            return;
        };
        flow.connections.retain(|connection| {
            if output {
                !(connection.from == id && connection.output == name)
            } else {
                !(connection.to == id && connection.input == name)
            }
        });
        if let Some(prefix) = numbered {
            for later in index + 1..length {
                flow.rename_port(
                    id,
                    output,
                    &format!("{prefix}{}", later + 1),
                    &format!("{prefix}{later}"),
                );
            }
        }
    }

    let Some(block) = flow.block_mut(id) else {
        return;
    };
    match (list, &mut block.kind) {
        (List::Variables, kind) => {
            if let Some(variables) = kind.variables_mut() {
                match remove {
                    Some(index) => {
                        variables.remove(index);
                    }
                    None => {
                        let name = (1..)
                            .map(|number| format!("value{number}"))
                            .find(|name| !variables.contains(name))
                            .unwrap_or_default();
                        variables.push(name);
                    }
                }
            }
        }
        (List::Conditions, BlockKind::Condition { conditions, .. }) => match remove {
            Some(index) => {
                conditions.remove(index);
            }
            None => conditions.push(String::new()),
        },
        (List::Fields, BlockKind::Record { fields }) => match remove {
            Some(index) => {
                fields.remove(index);
            }
            None => {
                let key = (1..)
                    .map(|number| format!("key{number}"))
                    .find(|key| !fields.iter().any(|field| field.key == *key))
                    .unwrap_or_default();
                fields.push(Field {
                    key,
                    value: String::new(),
                });
            }
        },
        (List::Items, BlockKind::List { items }) => match remove {
            Some(index) => {
                items.remove(index);
            }
            None => items.push(String::new()),
        },
        (List::Outputs, BlockKind::Output { names }) => match remove {
            Some(index) => {
                names.remove(index);
            }
            None => {
                let name = (1..)
                    .map(|number| format!("output{number}"))
                    .find(|name| !names.contains(name))
                    .unwrap_or_default();
                names.push(name);
            }
        },
        _ => {}
    }
}

fn section_title(title: &str) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(title.to_owned()))
}

fn labeled(label: &'static str, control: Option<AnyElement>) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(section_title(label))
        .children(control)
}

fn labeled_note(
    label: &'static str,
    control: Option<AnyElement>,
    note: &'static str,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(section_title(label))
        .children(control)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(note),
        )
}

fn list_row(control: Option<AnyElement>, remove: AnyElement) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(div().flex_1().min_w_0().children(control))
        .child(remove)
}

fn remove_button(id: &str, list: List, index: usize, cx: &Context<FlowEditor>) -> AnyElement {
    let id = id.to_owned();
    Button::new(SharedString::from(format!("flow-remove-{list:?}-{index}")))
        .xsmall()
        .ghost()
        .icon(Icon::new(IconName::Close).size_3())
        .accessibility_label("Remove")
        .on_click(cx.listener(move |this, _, window, cx| {
            this.change_list(&id, list, Some(index), window, cx)
        }))
        .into_any_element()
}

fn add_button(
    id: &str,
    list: List,
    label: &'static str,
    cx: &Context<FlowEditor>,
) -> impl IntoElement {
    let id = id.to_owned();
    Button::new(SharedString::from(format!("flow-add-{list:?}")))
        .xsmall()
        .ghost()
        .icon(IconName::Plus)
        .label(label)
        .on_click(
            cx.listener(move |this, _, window, cx| this.change_list(&id, list, None, window, cx)),
        )
}
