use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::Collection;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
    spinner::Spinner,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use import::{Import, ImportError};

use super::{
    panel::{CollectionPanel, CollectionPanelEvent},
    tree::path_name,
};

impl CollectionPanel {
    /// Opens a dialog that imports a Postman collection, from a file, a
    /// folder or pasted text, or an OpenAPI specification as a new
    /// collection. A pasted cURL command opens as a new request instead.
    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = cx.entity().downgrade();
        let dialog = cx.new(|cx| ImportDialog::new(panel, window, cx));
        let text = dialog.read(cx).text.clone();

        window.open_dialog(cx, move |modal, window, _| {
            // Wide like a page, but never wider than the window allows.
            let width = rems(48.)
                .to_pixels(window.rem_size())
                .min(window.viewport_size().width - rems(4.).to_pixels(window.rem_size()));
            let confirmed = dialog.downgrade();

            modal
                .title("Import")
                .w(width)
                // Enter imports the typed text. The dialog stays open to show
                // an error, and closes itself once the import is done.
                .on_ok(move |_, window, cx| {
                    let _ = confirmed.update(cx, |dialog, cx| {
                        let source = dialog.text.read(cx).value().to_string();
                        dialog.import_text(source, window, cx);
                    });
                    false
                })
                .child(dialog.clone())
        });
        text.update(cx, |text, cx| text.focus(window, cx));
    }

    /// Adds an imported collection and selects it. Its folders start
    /// collapsed, so a large import shows its structure first.
    pub fn add_imported_collection(
        &mut self,
        collection: Collection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = collection.path.clone();
        self.collections.add_collection(collection);
        self.rename = None;
        self.pending_delete = None;
        self.error = None;
        self.reveal(&path, None, window, cx);

        if let Some(index) = self.selected {
            let end = self.tree.items[index].end;
            self.collapsed
                .extend((index + 1..end).filter(|&child| self.tree.items[child].is_branch()));

            let rows = Arc::new(self.tree.visible_rows(&self.collapsed, ""));
            self.unfiltered_rows = Some(rows.clone());
            self.apply_rows(rows, false, cx);
        }

        if let Some(row) = self.selected_row() {
            self.scroll_handle.scroll_to_item(row, ScrollStrategy::Top);
        }
    }
}

/// The longest pasted cURL command that stays in the import field. Longer
/// ones, such as commands with large bodies, would be slow to lay out.
const RETAINED_COMMAND_LIMIT: usize = 16 * 1024;

/// Imports a Postman collection or an OpenAPI specification from a file or
/// pasted text, or a Postman collection folder, as a new collection.
struct ImportDialog {
    panel: WeakEntity<CollectionPanel>,
    /// Takes a cURL command or a collection's text, like Postman's import field.
    text: Entity<TextareaState>,
    importing: bool,
    error: Option<String>,
    /// The imported collection's name and the requests it left out, shown
    /// until the dialog is dismissed.
    skipped: Option<(String, Vec<String>)>,
    _task: Option<Task<()>>,
}

impl ImportDialog {
    fn new(
        panel: WeakEntity<CollectionPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Pasting imports at once. Typed text imports with Enter, and
        // Shift-Enter starts a new line.
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder("Paste cURL or raw text")
        });

        Self {
            panel,
            text,
            importing: false,
            error: None,
            skipped: None,
            _task: None,
        }
    }

    /// Opens a cURL command as a new request, or imports a collection
    /// written as text.
    fn import_text(&mut self, source: String, window: &mut Window, cx: &mut Context<Self>) {
        if source.trim().is_empty() {
            return;
        }

        if !import::is_curl(&source) {
            let read = move || {
                import::parse(&source).map_err(|error| match error {
                    ImportError::Syntax(error) => {
                        format!("The text is not valid JSON or YAML: {error}")
                    }
                    ImportError::UnknownFormat => "The text is not a cURL command, a Postman \
                                                   collection or an OpenAPI specification."
                        .to_owned(),
                    error => error.to_string(),
                })
            };
            self.import(read, window, cx);
            return;
        }

        // A pasted command stays in the field as it is, so a failed import can
        // be corrected and tried again with Enter. A collection is left out:
        // laying out a whole document would stall the window.
        if source.len() <= RETAINED_COMMAND_LIMIT && self.text.read(cx).value() != source {
            let text = source.clone();
            self.text
                .update(cx, |field, cx| field.set_value(text, window, cx));
        }

        match import::parse_curl(&source) {
            Ok(request) => {
                window.close_dialog(cx);
                let _ = self.panel.update(cx, |_, cx| {
                    cx.emit(CollectionPanelEvent::OpenUnsavedRequest(request))
                });
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Import".into()),
        });

        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = paths.await;

            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.import_file(path, window, cx);
                    }
                }
                Ok(Err(error)) => {
                    this.error = Some(format!("Could not open the file picker: {error}"));
                    cx.notify();
                }
                // The picker was cancelled.
                _ => {}
            });
        }));
    }

    fn import_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.import(
            move || import::read(&path).map_err(|error| error.to_string()),
            window,
            cx,
        );
    }

    /// Converts and writes a collection in the background.
    fn import(
        &mut self,
        read: impl FnOnce() -> Result<Import, String> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.panel.upgrade() else {
            return;
        };
        if self.importing {
            return;
        }

        self.importing = true;
        self.error = None;
        cx.notify();

        let directory = panel
            .read(cx)
            .collections
            .directory()
            .map(Path::to_path_buf);

        // Writing a large collection takes a while, so it happens here too.
        let imported = cx.background_executor().spawn(async move {
            let directory = directory.ok_or("No collections directory is configured.")?;
            let import = read()?;
            let collection = import
                .collection
                .write(&directory)
                .map_err(|error| format!("Could not import the collection: {error}"))?;

            Ok::<_, String>((collection, import.skipped))
        });

        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let imported = imported.await;

            let _ = this.update_in(cx, |this, window, cx| this.finish(imported, window, cx));
        }));
    }

    fn finish(
        &mut self,
        imported: Result<(Collection, Vec<String>), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.importing = false;

        let Some(panel) = self.panel.upgrade() else {
            return;
        };

        match imported {
            Ok((collection, skipped)) => {
                let name = path_name(&collection.path);
                panel.update(cx, |panel, cx| {
                    panel.add_imported_collection(collection, window, cx)
                });

                if skipped.is_empty() {
                    window.close_dialog(cx);
                    let focus = panel.read(cx).focus.clone();
                    window.focus(&focus, cx);
                } else {
                    self.skipped = Some((name, skipped));
                }
            }
            Err(error) => self.error = Some(error),
        }

        cx.notify();
    }

    fn render_summary(&self, name: &str, skipped: &[String], cx: &mut Context<Self>) -> Div {
        let explanation = match skipped.len() {
            1 => "1 request uses a protocol or HTTP method Request Eagle cannot send, so it was \
                  left out:"
                .to_owned(),
            count => format!(
                "{count} requests use protocols or HTTP methods Request Eagle cannot send, so \
                 they were left out:"
            ),
        };

        v_flex()
            .debug_selector(|| "import-summary".into())
            .gap_3()
            .text_sm()
            .child(format!("Imported “{name}”."))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(explanation),
            )
            .child(
                v_flex()
                    .id("import-skipped")
                    .max_h(rems(10.))
                    .overflow_y_scroll()
                    .gap_1()
                    .children(skipped.iter().map(|name| div().child(name.clone()))),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("close-import")
                        .debug_selector(|| "close-import".into())
                        .primary()
                        .label("Done")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

impl Render for ImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some((name, skipped)) = &self.skipped {
            return self.render_summary(name, skipped, cx);
        }

        let theme = cx.theme();

        let paste_target = cx.entity().downgrade();

        v_flex()
            .debug_selector(|| "import-dialog".into())
            .gap_3()
            .child(
                div().debug_selector(|| "import-text".into()).child(
                    Textarea::new(&self.text)
                        .aria_label("cURL command or raw text to import")
                        .disabled(self.importing)
                        // The pasted text replaces the field and is imported at once.
                        .on_paste(move |clipboard, window, cx| {
                            let Some(source) = clipboard.text() else {
                                return false;
                            };

                            paste_target
                                .update(cx, |this, cx| this.import_text(source, window, cx))
                                .is_ok()
                        }),
                ),
            )
            .child(
                v_flex()
                    .id("import-drop-zone")
                    .h(rems(20.))
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .p_6()
                    .rounded(theme.radius_tokens().md)
                    .border_1()
                    .border_dashed()
                    .border_color(theme.border)
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style
                            .border_color(cx.theme().ring)
                            .bg(cx.theme().accent.opacity(0.5))
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                        if let Some(path) = paths.paths().first() {
                            this.import_file(path.clone(), window, cx);
                        }
                    }))
                    .child(
                        h_flex()
                            .gap_3()
                            .child(if self.importing {
                                Spinner::new()
                                    .large()
                                    .color(theme.muted_foreground)
                                    .into_any_element()
                            } else {
                                Icon::default()
                                    .path("icons/import.svg")
                                    .size_6()
                                    .text_color(theme.muted_foreground)
                                    .into_any_element()
                            })
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(
                                        if self.importing {
                                            "Importing…"
                                        } else {
                                            "Drop a file or folder to import"
                                        },
                                    ))
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .text_color(theme.muted_foreground)
                                            .child("Or select")
                                            .child(
                                                Button::new("choose-import-file")
                                                    .debug_selector(|| "choose-import-file".into())
                                                    .link()
                                                    // Colored like a link, it needs no underline.
                                                    .text_decoration_0()
                                                    .label("a file or folder")
                                                    .disabled(self.importing)
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_file(window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            div()
                                .debug_selector(|| "import-error".into())
                                .max_w(rems(30.))
                                .text_center()
                                .text_sm()
                                .text_color(theme.danger)
                                .child(error),
                        )
                    }),
            )
            // The formats wrap onto separate lines in narrow windows.
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_6()
                    .gap_y_1()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("cURL commands")
                    .child("Postman Collection v2.0 and v2.1")
                    .child("Postman collection folders, with gRPC requests")
                    .child("OpenAPI 3 and Swagger 2.0, in JSON or YAML"),
            )
    }
}
