use std::path::{Path, PathBuf};

use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Body, Method, RawLanguage};

use super::draft::{RequestDraft, RequestLocation};
use super::fields::{ChooseFile, FieldsChanged, RequestFields};
use crate::variable_input::{VariableInput, VariableTarget, with_variables};

/// No body, or the type of the body the section edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyType {
    None,
    Raw(RawLanguage),
    UrlEncoded,
    Multipart,
    Binary,
}

impl BodyType {
    /// The menu's options, in groups.
    const GROUPS: [&[Self]; 4] = [
        &[Self::None],
        &[
            Self::Raw(RawLanguage::Json),
            Self::Raw(RawLanguage::Xml),
            Self::Raw(RawLanguage::Text),
        ],
        &[Self::UrlEncoded, Self::Multipart],
        &[Self::Binary],
    ];

    fn of(body: Option<&Body>) -> Self {
        match body {
            None => Self::None,
            Some(Body::Raw { language, .. }) => Self::Raw(*language),
            Some(Body::UrlEncoded { .. }) => Self::UrlEncoded,
            Some(Body::Multipart { .. }) => Self::Multipart,
            Some(Body::Binary { .. }) => Self::Binary,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "No Body",
            Self::Raw(RawLanguage::Json) => "JSON",
            Self::Raw(RawLanguage::Xml) => "XML",
            Self::Raw(RawLanguage::Text) => "Text",
            Self::UrlEncoded => "Form URL Encoded",
            Self::Multipart => "Multipart Form",
            Self::Binary => "Binary File",
        }
    }
}

/// The editor's highlighting language and placeholder for raw text.
fn editor_language(language: RawLanguage) -> (&'static str, &'static str) {
    match language {
        RawLanguage::Json => ("json", "Enter JSON request body"),
        RawLanguage::Xml => {
            crate::response_view::register_xml();
            ("xml", "Enter XML request body")
        }
        RawLanguage::Text => ("text", "Enter request body"),
    }
}

impl RequestDraft {
    pub(super) fn supports_body(&self) -> bool {
        !matches!(self.request.method, Method::Get | Method::Head)
    }

    /// Create the editor of the body's type.
    pub(super) fn body_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match BodyType::of(self.request.body.as_ref()) {
            BodyType::None => {}
            BodyType::Raw(_) => {
                self.raw_body_state(window, cx);
            }
            BodyType::UrlEncoded => {
                self.form_state(window, cx);
            }
            BodyType::Multipart => {
                self.parts_state(window, cx);
            }
            BodyType::Binary => {
                self.body_file_state(window, cx);
            }
        }
    }

    /// Show the file paths of the body as saved, which saving can make
    /// relative to the collection. The editors keep everything else.
    pub fn set_saved_files(
        &mut self,
        body: Option<Body>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.request.body == body {
            return;
        }

        match &body {
            Some(Body::Binary { file }) => {
                if let Some(input) = &self.body_file {
                    let path = file.to_string_lossy().into_owned();
                    input.update(cx, |input, cx| input.set_value(path, window, cx));
                }
            }
            Some(Body::Multipart { parts }) => {
                if let Some(table) = &self.parts {
                    table.update(cx, |table, cx| table.set_files(parts, window, cx));
                }
            }
            _ => {}
        }
        self.request.body = body;

        self.refresh_generated_headers(cx);
        cx.notify();
    }

    /// Change the type of the body. Each type's editor keeps what was
    /// entered in it, so switching back restores it.
    fn set_body_type(&mut self, body_type: BodyType, window: &mut Window, cx: &mut Context<Self>) {
        if body_type == BodyType::of(self.request.body.as_ref()) {
            return;
        }

        let text = |editor: &Option<Entity<EditorState>>, cx: &App| {
            editor
                .as_ref()
                .map(|editor| editor.read(cx).value().to_string())
                .unwrap_or_default()
        };
        self.request.body = match body_type {
            BodyType::None => None,
            BodyType::Raw(language) => Some(Body::Raw {
                language,
                text: text(&self.body, cx),
            }),
            BodyType::UrlEncoded => Some(Body::UrlEncoded {
                fields: self
                    .form
                    .as_ref()
                    .map(|form| form.read(cx).values(cx))
                    .unwrap_or_default(),
            }),
            BodyType::Multipart => Some(Body::Multipart {
                parts: self
                    .parts
                    .as_ref()
                    .map(|parts| parts.read(cx).parts(cx))
                    .unwrap_or_default(),
            }),
            BodyType::Binary => Some(Body::Binary {
                file: self
                    .body_file
                    .as_ref()
                    .map(|file| file.read(cx).value().to_string().into())
                    .unwrap_or_default(),
            }),
        };

        if let (BodyType::Raw(language), Some(editor)) = (body_type, &self.body) {
            let (name, placeholder) = editor_language(language);
            editor.update(cx, |editor, cx| {
                editor.set_highlighter(name, cx);
                editor.set_placeholder(placeholder, window, cx);
            });
            let value = editor.read(cx).value();
            self.validate_body(value, cx);
        }

        self.body_state(window, cx);
        self.refresh_generated_headers(cx);
        cx.notify();
    }

    fn raw_body_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        if let Some(body) = &self.body {
            return body.clone();
        }

        let (language, value) = match &self.request.body {
            Some(Body::Raw { language, text }) => (*language, text.clone()),
            _ => (RawLanguage::Json, String::new()),
        };
        let (name, placeholder) = editor_language(language);
        let body = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(name)
                .line_number(true)
                .soft_wrap(true)
                .placeholder(placeholder)
                .default_value(value)
        });
        let scope = self.variables.clone();
        self.body_vim = Some(cx.new(|cx| crate::vim::Vim::new(body.clone(), cx)));
        self.body_completion =
            Some(cx.new(|cx| {
                VariableInput::new(VariableTarget::Editor(body.clone()), scope, window, cx)
            }));
        self._subscriptions
            .push(cx.subscribe(&body, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    if let Some(Body::Raw { text, .. }) = &mut this.request.body {
                        *text = value.to_string();
                    }
                    this.validate_body(value, cx);
                    this.refresh_generated_headers(cx);
                    cx.notify();
                }
            }));
        self.body = Some(body.clone());
        self.validate_body(body.read(cx).value(), cx);

        body
    }

    fn form_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<RequestFields> {
        if let Some(form) = &self.form {
            return form.clone();
        }

        let fields = match &self.request.body {
            Some(Body::UrlEncoded { fields }) => fields.clone(),
            _ => Vec::new(),
        };
        let scope = self.variables.clone();
        // A field without a name is sent as `=value`.
        let form = cx.new(|cx| {
            RequestFields::new("form", &fields, &[], scope, window, cx).with_keyless_rows()
        });
        self._subscriptions
            .push(cx.subscribe(&form, |this, _, event: &FieldsChanged, cx| {
                if let Some(Body::UrlEncoded { fields }) = &mut this.request.body {
                    *fields = event.0.clone();
                }
                this.refresh_generated_headers(cx);
                cx.notify();
            }));
        self.form = Some(form.clone());

        form
    }

    fn parts_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        if let Some(parts) = &self.parts {
            return parts.clone();
        }

        let values = match &self.request.body {
            Some(Body::Multipart { parts }) => parts.clone(),
            _ => Vec::new(),
        };
        let scope = self.variables.clone();
        let parts = cx.new(|cx| RequestFields::multipart("multipart", &values, scope, window, cx));
        self._subscriptions
            .push(cx.subscribe(&parts, |this, fields, _: &FieldsChanged, cx| {
                if let Some(Body::Multipart { parts }) = &mut this.request.body {
                    *parts = fields.read(cx).parts(cx);
                }
                this.refresh_generated_headers(cx);
                cx.notify();
            }));
        self._subscriptions.push(cx.subscribe_in(
            &parts,
            window,
            |this, _, ChooseFile(input), window, cx| this.choose_file(input.clone(), window, cx),
        ));
        self.parts = Some(parts.clone());

        parts
    }

    fn body_file_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(file) = &self.body_file {
            return file.clone();
        }

        let path = match &self.request.body {
            Some(Body::Binary { file }) => file.to_string_lossy().into_owned(),
            _ => String::new(),
        };
        let file = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Choose a file")
                .default_value(path)
        });
        self._subscriptions
            .push(cx.subscribe(&file, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(Body::Binary { file }) = &mut this.request.body {
                        *file = input.read(cx).value().to_string().into();
                    }
                    this.refresh_generated_headers(cx);
                    cx.notify();
                }
            }));
        self.body_file = Some(file.clone());

        file
    }

    /// Put the path of a chosen file in `input`. A file inside the
    /// collection is stored relative to it, so the collection keeps working
    /// when it is shared or moved.
    fn choose_file(
        &mut self,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let _ = this.update_in(cx, |this, window, cx| {
                let path = this.stored_path(path);
                input.update(cx, |input, cx| {
                    input.replace_all(path.to_string_lossy().into_owned(), window, cx)
                });
            });
        })
        .detach();
    }

    fn stored_path(&self, path: PathBuf) -> PathBuf {
        self.location
            .as_ref()
            .and_then(RequestLocation::collection_path)
            .and_then(|collection| path.strip_prefix(collection).ok().map(Path::to_path_buf))
            .unwrap_or(path)
    }

    fn validate_body(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.body_json_valid = false;

        // A new edit drops the previous validation/formatting task, so an old
        // result cannot enable Format or overwrite a more recent body.
        let task = cx
            .background_executor()
            .spawn(async move { serde_json::from_str::<serde_json::Value>(&text).is_ok() });
        self.body_task = Some(cx.spawn(async move |this, cx| {
            let valid = task.await;
            let _ = this.update(cx, |this, cx| {
                this.body_json_valid = valid;
                this.body_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn format_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.body_json_valid || self.body_task.is_some() {
            return;
        }

        let Some(body) = &self.body else { return };
        let text = body.read(cx).value();
        let source = text.clone();
        let task = cx.background_executor().spawn(async move {
            serde_json::from_str::<serde_json::Value>(&text)
                .and_then(|value| serde_json::to_string_pretty(&value))
        });
        self.body_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.body_task = None;

                if let Ok(text) = result
                    && let Some(body) = &this.body
                    && body.read(cx).value() == source
                {
                    body.update(cx, |body, cx| body.replace_all(text, window, cx));
                }

                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.body_state(window, cx);
        let body_type = BodyType::of(self.request.body.as_ref());
        let draft = cx.entity().downgrade();

        let content = match body_type {
            BodyType::None => div()
                .debug_selector(|| "request-body-none".into())
                .py_4()
                .text_color(cx.theme().muted_foreground)
                .child("This request does not have a body.")
                .into_any_element(),
            BodyType::Raw(language) => self.raw_body(language, window, cx),
            BodyType::UrlEncoded => self.form_state(window, cx).into_any_element(),
            BodyType::Multipart => self.parts_state(window, cx).into_any_element(),
            BodyType::Binary => self.binary_body(window, cx),
        };

        v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .flex_none()
                    .h_7()
                    .gap_2()
                    .child(
                        Button::new("request-body-type")
                            .debug_selector(|| "request-body-type".into())
                            .ghost()
                            .small()
                            .label(body_type.label())
                            .icon(IconName::ChevronDown)
                            .accessibility_label(format!("Body type: {}", body_type.label()))
                            .dropdown_menu(move |menu, _, _| {
                                BodyType::GROUPS.iter().enumerate().fold(
                                    menu,
                                    |menu, (index, group)| {
                                        let menu = if index > 0 { menu.separator() } else { menu };
                                        group.iter().fold(menu, |menu, &option| {
                                            let draft = draft.clone();
                                            menu.item(
                                                PopupMenuItem::new(option.label())
                                                    .checked(option == body_type)
                                                    .on_click(move |_, window, cx| {
                                                        let _ = draft.update(cx, |draft, cx| {
                                                            draft.set_body_type(option, window, cx)
                                                        });
                                                    }),
                                            )
                                        })
                                    },
                                )
                            }),
                    )
                    .child(div().flex_1())
                    .when(matches!(body_type, BodyType::Raw(_)), |row| {
                        row.children(self.body_vim.clone())
                    })
                    .when(body_type == BodyType::Raw(RawLanguage::Json), |row| {
                        row.child(
                            Button::new("format-request-json")
                                .debug_selector(|| "format-request-json".into())
                                .ghost()
                                .small()
                                .label("Format")
                                .disabled(!self.body_json_valid || self.body_task.is_some())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.format_body(window, cx)),
                                ),
                        )
                    }),
            )
            .child(content)
            .into_any_element()
    }

    fn raw_body(
        &mut self,
        language: RawLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = self.raw_body_state(window, cx);
        let vim = self.body_vim.clone().unwrap();
        let mouse_vim = vim.clone();
        let label = match language {
            RawLanguage::Json => "JSON request body",
            RawLanguage::Xml => "XML request body",
            RawLanguage::Text => "Request body",
        };

        div()
            .debug_selector(|| "request-body".into())
            .track_focus(&vim.focus_handle(cx))
            .capture_any_mouse_down(move |_, _, cx| {
                mouse_vim.update(cx, |vim, _| vim.mouse_down());
            })
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                with_variables(
                    self.body_completion.as_ref().unwrap(),
                    Editor::new(&body)
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
                        .aria_label(label),
                )
                .h_full(),
            )
            .child(crate::vim::cursor(&vim))
            .into_any_element()
    }

    fn binary_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let file = self.body_file_state(window, cx);

        v_flex()
            .gap_2()
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Sends the file's contents as the body. Paths inside the collection are saved relative to it."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "request-body-file".into())
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&file).small().aria_label("Body file")),
                    )
                    .child(
                        Button::new("request-body-choose-file")
                            .debug_selector(|| "request-body-choose-file".into())
                            .outline()
                            .small()
                            .label("Choose a File")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_file(file.clone(), window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
}
