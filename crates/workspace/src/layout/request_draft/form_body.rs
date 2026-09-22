use std::path::PathBuf;

use gpui_kit::component::{
    button::*,
    input::{InputEvent, Textarea, TextareaState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{FormBody, MultipartField};

use super::draft::RequestDraft;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyMode {
    Raw,
    UrlEncoded,
    Multipart,
}

impl RequestDraft {
    pub(super) fn prepare_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request.form.is_some() {
            self.form_state(window, cx);
        } else {
            self.body_state(window, cx);
        }
    }

    pub(super) fn body_modes(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = match self.request.form {
            None => BodyMode::Raw,
            Some(FormBody::UrlEncoded(_)) => BodyMode::UrlEncoded,
            Some(FormBody::Multipart(_)) => BodyMode::Multipart,
        };

        h_flex().flex_none().gap_1().children(
            [
                (BodyMode::Raw, "raw", "Raw JSON"),
                (BodyMode::UrlEncoded, "urlencoded", "Form URL-encoded"),
                (BodyMode::Multipart, "multipart", "Multipart form"),
            ]
            .into_iter()
            .map(|(mode, id, label)| {
                Button::new(id)
                    .debug_selector(move || format!("request-body-mode-{id}"))
                    .small()
                    .ghost()
                    .label(label)
                    .when(mode == selected, |button| button.bg(cx.theme().muted))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_body_mode(mode, window, cx);
                    }))
            }),
        )
    }

    pub(super) fn set_body_mode(
        &mut self,
        mode: BodyMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request.form = match mode {
            BodyMode::Raw => None,
            BodyMode::UrlEncoded => Some(
                self.url_form
                    .as_ref()
                    .map(|form| form.read(cx).value(cx))
                    .unwrap_or_else(|| FormBody::UrlEncoded(Vec::new())),
            ),
            BodyMode::Multipart => Some(
                self.multipart_form
                    .as_ref()
                    .map(|form| form.read(cx).value(cx))
                    .unwrap_or_else(|| FormBody::Multipart(Vec::new())),
            ),
        };
        self.prepare_body(window, cx);
        self.refresh_generated_headers(cx);
        cx.notify();
    }

    pub(super) fn form_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FormEditor> {
        let form = self.request.form.as_ref().expect("active form body");
        let multipart = matches!(form, FormBody::Multipart(_));
        let slot = if multipart {
            &mut self.multipart_form
        } else {
            &mut self.url_form
        };

        if slot.is_none() {
            let editor = cx.new(|cx| FormEditor::new(form, window, cx));
            self._subscriptions.push(cx.subscribe(
                &editor,
                move |this, _, event: &FormChanged, cx| {
                    let active =
                        matches!(this.request.form, Some(FormBody::Multipart(_))) == multipart;

                    if this.request.form.is_some() && active {
                        this.request.form = Some(event.0.clone());
                        this.refresh_generated_headers(cx);
                        cx.notify();
                    }
                },
            ));
            *slot = Some(editor);
        }

        slot.as_ref().unwrap().clone()
    }
}

struct FormRow {
    name: Entity<TextareaState>,
    value: Entity<TextareaState>,
    file: bool,
    _subscriptions: Vec<Subscription>,
}

pub(super) struct FormChanged(FormBody);

pub(super) struct FormEditor {
    multipart: bool,
    rows: Vec<FormRow>,
}

impl EventEmitter<FormChanged> for FormEditor {}

impl FormEditor {
    fn new(form: &FormBody, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut editor = Self {
            multipart: matches!(form, FormBody::Multipart(_)),
            rows: Vec::new(),
        };

        match form {
            FormBody::UrlEncoded(fields) => {
                for (name, value) in fields {
                    editor.append(name, value, false, window, cx);
                }
            }
            FormBody::Multipart(fields) => {
                for field in fields {
                    match field {
                        MultipartField::Text { name, value } => {
                            editor.append(name, value, false, window, cx)
                        }
                        MultipartField::File { name, path } => {
                            editor.append(name, &path.to_string_lossy(), true, window, cx)
                        }
                    }
                }
            }
        }

        if editor.rows.is_empty() {
            editor.append("", "", false, window, cx);
        }

        editor
    }

    fn append(
        &mut self,
        name: &str,
        value: &str,
        file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 4)
                .placeholder("Name")
                .default_value(name.to_owned())
        });
        let value = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 4)
                .placeholder(if file {
                    "Select a file or enter its path"
                } else {
                    "Value"
                })
                .default_value(value.to_owned())
        });
        let subscriptions = [&name, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.emit_change(cx);
                    }
                })
            })
            .collect();

        self.rows.push(FormRow {
            name,
            value,
            file,
            _subscriptions: subscriptions,
        });
    }

    pub(super) fn value(&self, cx: &App) -> FormBody {
        if self.multipart {
            FormBody::Multipart(
                self.rows
                    .iter()
                    .filter_map(|row| {
                        let name = row.name.read(cx).value().to_string();
                        let value = row.value.read(cx).value().to_string();

                        if name.is_empty() && value.is_empty() {
                            return None;
                        }

                        Some(if row.file {
                            MultipartField::File {
                                name,
                                path: PathBuf::from(value),
                            }
                        } else {
                            MultipartField::Text { name, value }
                        })
                    })
                    .collect(),
            )
        } else {
            FormBody::UrlEncoded(
                self.rows
                    .iter()
                    .filter_map(|row| {
                        let name = row.name.read(cx).value().to_string();
                        let value = row.value.read(cx).value().to_string();

                        (!name.is_empty() || !value.is_empty()).then_some((name, value))
                    })
                    .collect(),
            )
        }
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        cx.emit(FormChanged(self.value(cx)));
        cx.notify();
    }

    fn browse(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.rows[index].value.clone();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose upload file".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = paths
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|paths| paths.into_iter().next())
            else {
                return;
            };

            let _ = this.update_in(cx, |this, window, cx| {
                // A row may be removed while the native picker is open.
                if this.rows.iter().any(|row| row.value == input) {
                    input.update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    });
                    this.emit_change(cx);
                }
            });
        })
        .detach();
    }
}

impl Render for FormEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_2()
            .debug_selector(|| "request-form".into())
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.multipart {
                        "Add text fields or files. Files are read on send. Form headers are set automatically."
                    } else {
                        "Names and values are URL-encoded on send. Form headers are set automatically."
                    }),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .w(px(52.))
                            .flex_none()
                            .text_color(cx.theme().muted_foreground)
                            .child(if row.file { "File" } else { "Text" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(move || format!("form-name-{index}"))
                            .child(
                                Textarea::new(&row.name)
                                    .text_size(px(13.))
                                    .aria_label(format!("Form field name {}", index + 1)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(move || format!("form-value-{index}"))
                            .child(Textarea::new(&row.value).text_size(px(13.)).aria_label(format!(
                                "Form field {} {}",
                                if row.file { "file path" } else { "value" },
                                index + 1
                            ))),
                    )
                    .when(row.file, |view| {
                        view.child(
                            Button::new(("browse", index))
                                .small()
                                .label("Browse")
                                .debug_selector(move || format!("form-browse-{index}"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.browse(index, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new(("remove", index))
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .debug_selector(move || format!("form-remove-{index}"))
                            .accessibility_label("Remove form field")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.rows.remove(index);
                                this.emit_change(cx);
                            })),
                    )
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("add-form-text")
                            .small()
                            .label("Add text field")
                            .debug_selector(|| "add-form-text".into())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.append("", "", false, window, cx);
                                cx.notify();
                            })),
                    )
                    .when(self.multipart, |view| {
                        view.child(
                            Button::new("add-form-file")
                                .small()
                                .label("Add file")
                                .debug_selector(|| "add-form-file".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.append("", "", true, window, cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }
}
