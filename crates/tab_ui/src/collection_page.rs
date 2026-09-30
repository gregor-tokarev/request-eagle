use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    tag::Tag,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::RequestScripts;

use crate::script_editor::{ScriptEditor, ScriptTarget, ScriptsChanged};
use crate::variable_table::{VariableTable, VariablesChanged};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionSection {
    Variables,
    Scripts,
}

/// A collection's editable settings. Variables keep the order they are edited in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollectionSettings {
    pub name: String,
    pub variables: Vec<(String, String)>,
    pub scripts: RequestScripts,
}

/// Emitted by the page's Save button. The workspace owns collection storage.
pub struct SaveCollection;

/// A collection's name, variables and scripts, edited in one tab and saved together.
pub struct CollectionPage {
    pub path: PathBuf,
    saved: CollectionSettings,
    pub(crate) draft: CollectionSettings,
    pub(crate) section: CollectionSection,
    name: Option<Entity<InputState>>,
    variables: Option<Entity<VariableTable>>,
    pub(crate) scripts: Option<Entity<ScriptEditor>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SaveCollection> for CollectionPage {}

impl CollectionPage {
    pub fn new(
        path: PathBuf,
        name: String,
        variables: HashMap<String, String>,
        scripts: RequestScripts,
    ) -> Self {
        let mut variables: Vec<_> = variables.into_iter().collect();
        variables.sort();
        let saved = CollectionSettings {
            name,
            variables,
            scripts,
        };

        Self {
            path,
            draft: saved.clone(),
            saved,
            section: CollectionSection::Variables,
            name: None,
            variables: None,
            scripts: None,
            _subscriptions: Vec::new(),
        }
    }

    /// The saved name, which the workspace shows as the tab title.
    pub fn name(&self) -> &str {
        &self.saved.name
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.saved
    }

    /// The edits to save, or why they cannot be saved yet.
    pub fn settings(&self) -> Result<CollectionSettings, String> {
        match variable_error(&self.draft.variables) {
            Some(error) => Err(error),
            None => Ok(self.draft.clone()),
        }
    }

    pub fn mark_saved(
        &mut self,
        path: PathBuf,
        settings: CollectionSettings,
        cx: &mut Context<Self>,
    ) {
        self.path = path;
        self.saved = settings;
        cx.notify();
    }

    /// Follow a rename made in the sidebar. An unsaved name edit is kept.
    pub fn relocate(
        &mut self,
        path: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.path = path;

        if self.draft.name == self.saved.name {
            self.draft.name = name.clone();

            if let Some(input) = &self.name {
                input.update(cx, |input, cx| input.set_value(name.clone(), window, cx));
            }
        }

        self.saved.name = name;
        cx.notify();
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_state(window, cx);

        match self.section {
            CollectionSection::Variables => {
                self.variables_state(window, cx);
            }
            CollectionSection::Scripts => {
                self.script_editor(cx)
                    .update(cx, |scripts, cx| scripts.editor(window, cx));
            }
        }
    }

    fn name_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        self.name
            .get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Collection name")
                        .default_value(self.draft.name.clone())
                });
                self._subscriptions.push(cx.subscribe(
                    &input,
                    |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.draft.name = input.read(cx).value().trim().to_owned();
                            cx.notify();
                        }
                    },
                ));

                input
            })
            .clone()
    }

    fn variables_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<VariableTable> {
        self.variables
            .get_or_insert_with(|| {
                let table = cx.new(|cx| {
                    VariableTable::new(
                        "collection-variable",
                        "name",
                        &self.draft.variables,
                        window,
                        cx,
                    )
                });
                self._subscriptions.push(cx.subscribe(
                    &table,
                    |this, _, event: &VariablesChanged, cx| {
                        this.draft.variables = event.0.clone();
                        cx.notify();
                    },
                ));

                table
            })
            .clone()
    }

    pub(crate) fn script_editor(&mut self, cx: &mut Context<Self>) -> Entity<ScriptEditor> {
        self.scripts
            .get_or_insert_with(|| {
                let scripts = cx.new(|_| {
                    ScriptEditor::new(self.draft.scripts.clone(), ScriptTarget::Collection)
                });
                self._subscriptions.push(cx.subscribe(
                    &scripts,
                    |this, _, event: &ScriptsChanged, cx| {
                        this.draft.scripts = event.0.clone();
                        cx.notify();
                    },
                ));

                scripts
            })
            .clone()
    }

    fn header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = self.name_state(window, cx);

        h_flex()
            .flex_none()
            .h_10()
            .gap_2()
            .child(
                Tag::secondary()
                    .small()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Collection"),
            )
            .child(
                div()
                    .debug_selector(|| "collection-name".into())
                    .flex_1()
                    .min_w_0()
                    .max_w(rems(28.))
                    .child(Input::new(&name).aria_label("Collection name")),
            )
            .child(div().flex_1())
            .child(
                Button::new("save-collection")
                    .debug_selector(|| "save-collection".into())
                    .primary()
                    .min_w_20()
                    .flex_none()
                    .label("Save")
                    .disabled(!self.is_dirty())
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SaveCollection))),
            )
    }

    fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scripts = usize::from(!self.draft.scripts.pre_request.is_empty())
            + usize::from(!self.draft.scripts.post_response.is_empty());
        let sections = [
            (
                "Variables",
                CollectionSection::Variables,
                self.draft.variables.len(),
            ),
            ("Scripts", CollectionSection::Scripts, scripts),
        ];

        Tabs::new("collection-sections")
            .flex()
            .flex_none()
            .gap_1()
            .children(sections.into_iter().map(|(label, section, count)| {
                let selected = section == self.section;

                Tab::new(label)
                    .debug_selector(move || format!("collection-section-{label}"))
                    .selected(selected)
                    .accessibility_label(label)
                    .flex_none()
                    .h_8()
                    .px_2()
                    .gap_1()
                    .rounded(cx.theme().radius_tokens().md)
                    .text_color(cx.theme().muted_foreground)
                    .hover(|this| this.bg(cx.theme().muted))
                    .when(selected, |this| {
                        this.bg(cx.theme().muted).text_color(cx.theme().foreground)
                    })
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

    fn variables_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let table = self.variables_state(window, cx);

        v_flex()
            .gap_2()
            .pb_3()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Use {{name}} in any request in this collection. \
                         Values that scripts set last until the app closes.",
                    ),
            )
            .child(table)
            .when_some(variable_error(&self.draft.variables), |this, error| {
                this.child(
                    div()
                        .debug_selector(|| "collection-variable-error".into())
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .into_any_element()
    }
}

fn variable_error(variables: &[(String, String)]) -> Option<String> {
    let mut names = HashSet::new();

    for (name, _) in variables {
        if !environment::valid_variable_name(name) {
            return Some(format!(
                "“{name}” is not a valid variable name. Use letters, numbers, _, - and ."
            ));
        }

        if !names.insert(name) {
            return Some(format!("“{name}” is defined more than once."));
        }
    }

    None
}

impl Render for CollectionPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.section {
            CollectionSection::Variables => self.variables_section(window, cx),
            CollectionSection::Scripts => self.script_editor(cx).into_any_element(),
        };

        v_flex()
            .debug_selector(|| "collection-page".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_sm()
            .child(self.header(window, cx))
            .child(self.section_tabs(cx))
            .child(
                div()
                    .id("collection-section-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(content),
            )
    }
}
