use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::Collection;
use environment::GlobalEnvironments;
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
    /// Opens a dialog that imports Postman collections and environments, from
    /// files, folders or pasted text, and OpenAPI specifications. Collections
    /// are added as new collections and environments to `environments`. A
    /// pasted cURL command opens as a new request instead.
    pub fn open_import_dialog(
        &mut self,
        environments: GlobalEnvironments,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity().downgrade();
        let dialog = cx.new(|cx| ImportDialog::new(panel, environments, window, cx));
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

    /// Adds imported collections and selects the first. Their folders start
    /// collapsed, so a large import shows its structure first. Several
    /// collections start collapsed too, so they show as a list.
    pub fn add_imported_collections(
        &mut self,
        collections: Vec<Collection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<_> = collections
            .iter()
            .map(|collection| collection.path.clone())
            .collect();
        let Some(first) = paths.first() else {
            return;
        };

        for collection in collections {
            self.collections.add_collection(collection);
        }
        self.rename = None;
        self.pending_delete = None;
        self.error = None;
        self.reveal(first, None, window, cx);

        let several = paths.len() > 1;
        for path in &paths {
            let Some(index) = self.tree.items.iter().position(|item| &item.path == path) else {
                continue;
            };

            let start = if several { index } else { index + 1 };
            let end = self.tree.items[index].end;
            self.collapsed
                .extend((start..end).filter(|&item| self.tree.items[item].is_branch()));
        }

        let rows = Arc::new(self.tree.visible_rows(&self.collapsed, ""));
        self.unfiltered_rows = Some(rows.clone());
        self.apply_rows(rows, false, cx);

        if let Some(row) = self.selected_row() {
            self.scroll_handle.scroll_to_item(row, ScrollStrategy::Top);
        }
    }
}

/// The longest pasted cURL command that stays in the import field. Longer
/// ones, such as commands with large bodies, would be slow to lay out.
const RETAINED_COMMAND_LIMIT: usize = 16 * 1024;

/// What to import.
enum Source {
    Text(String),
    /// Files and folders. A Postman workspace folder stands for each of its
    /// collections and environments.
    Paths(Vec<PathBuf>),
}

/// What an import wrote, and what it could not.
#[derive(Default)]
struct Outcome {
    collections: Vec<Collection>,
    /// The names the environments were saved under.
    environments: Vec<String>,
    /// Requests left out because Request Eagle cannot send their protocol or
    /// HTTP method, each with the name of its collection.
    skipped: Vec<(String, String)>,
    /// Why each file or folder could not be imported.
    failed: Vec<String>,
}

/// What an import that left something out brought in, shown until the dialog
/// is dismissed.
struct Summary {
    collections: Vec<String>,
    environments: Vec<String>,
    skipped: Vec<String>,
    failed: Vec<String>,
}

/// Imports Postman collections and environments and OpenAPI specifications
/// from files, folders or pasted text.
struct ImportDialog {
    panel: WeakEntity<CollectionPanel>,
    environments: GlobalEnvironments,
    /// Takes a cURL command or a collection's text, like Postman's import field.
    text: Entity<TextareaState>,
    importing: bool,
    error: Option<String>,
    summary: Option<Summary>,
    _task: Option<Task<()>>,
}

impl ImportDialog {
    fn new(
        panel: WeakEntity<CollectionPanel>,
        environments: GlobalEnvironments,
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
            environments,
            text,
            importing: false,
            error: None,
            summary: None,
            _task: None,
        }
    }

    /// Opens a cURL command as a new request, or imports a collection or
    /// environment written as text.
    fn import_text(&mut self, source: String, window: &mut Window, cx: &mut Context<Self>) {
        if source.trim().is_empty() {
            return;
        }

        if !import::is_curl(&source) {
            self.import(Source::Text(source), window, cx);
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
            multiple: true,
            prompt: Some("Import".into()),
        });

        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = paths.await;

            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(paths))) => this.import(Source::Paths(paths), window, cx),
                Ok(Err(error)) => {
                    this.error = Some(format!("Could not open the file picker: {error}"));
                    cx.notify();
                }
                // The picker was cancelled.
                _ => {}
            });
        }));
    }

    /// Converts and writes collections and environments in the background.
    fn import(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.panel.upgrade() else {
            return;
        };
        if self.importing || matches!(&source, Source::Paths(paths) if paths.is_empty()) {
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
        let environments = self.environments.clone();

        // Reading and writing a large collection takes a while.
        let outcome = cx.background_executor().spawn(async move {
            let reads: Vec<_> = match source {
                Source::Text(text) => vec![(None, read_text(&text))],
                Source::Paths(paths) => paths
                    .iter()
                    .flat_map(|path| import::sources(path))
                    .map(|path| {
                        let read = import::read(&path).map_err(|error| error.to_string());
                        (Some(path_name(&path)), read)
                    })
                    .collect(),
            };

            // Failures name their file when there are several.
            let several = reads.len() > 1;
            let mut outcome = Outcome::default();
            for (name, read) in reads {
                let saved = read.and_then(|import| {
                    save(import, directory.as_deref(), &environments, &mut outcome)
                });

                if let Err(error) = saved {
                    outcome.failed.push(match name {
                        Some(name) if several => format!("{name}: {error}"),
                        _ => error,
                    });
                }
            }

            outcome
        });

        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome = outcome.await;

            let _ = this.update_in(cx, |this, window, cx| this.finish(outcome, window, cx));
        }));
    }

    fn finish(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        self.importing = false;

        let Some(panel) = self.panel.upgrade() else {
            return;
        };

        let Outcome {
            collections,
            environments,
            skipped,
            failed,
        } = outcome;

        // Nothing was written, so the dialog stays open to try again.
        if collections.is_empty() && environments.is_empty() {
            self.error = Some(failed.join("\n"));
            cx.notify();
            return;
        }

        let names: Vec<_> = collections
            .iter()
            .map(|collection| path_name(&collection.path))
            .collect();
        panel.update(cx, |panel, cx| {
            panel.add_imported_collections(collections, window, cx)
        });
        if !environments.is_empty() {
            panel.update(cx, |_, cx| {
                cx.emit(CollectionPanelEvent::EnvironmentsImported)
            });
        }

        if skipped.is_empty() && failed.is_empty() {
            window.close_dialog(cx);
            let focus = panel.read(cx).focus.clone();
            window.focus(&focus, cx);
        } else {
            // Requests from several collections are named with their collection.
            let skipped = skipped
                .into_iter()
                .map(|(collection, request)| {
                    if names.len() > 1 {
                        format!("{collection} / {request}")
                    } else {
                        request
                    }
                })
                .collect();

            self.summary = Some(Summary {
                collections: names,
                environments,
                skipped,
                failed,
            });
        }

        cx.notify();
    }

    fn render_summary(&self, summary: &Summary, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let list = |id: &'static str, items: &[String]| {
            v_flex()
                .id(id)
                .max_h(rems(10.))
                .overflow_y_scroll()
                .gap_1()
                .children(items.iter().map(|item| div().child(item.clone())))
        };
        let skipped = match summary.skipped.len() {
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
            .child(summary.heading())
            .when(!summary.skipped.is_empty(), |this| {
                this.child(div().text_color(muted).child(skipped))
                    .child(list("import-skipped", &summary.skipped))
            })
            .when(!summary.failed.is_empty(), |this| {
                this.child(div().text_color(muted).child("Could not import:"))
                    .child(list("import-failed", &summary.failed))
            })
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

impl Summary {
    fn heading(&self) -> String {
        if let [name] = &[&self.collections[..], &self.environments[..]].concat()[..] {
            return format!("Imported “{name}”.");
        }

        let count = |count: usize, noun: &str| match count {
            1 => format!("1 {noun}"),
            count => format!("{count} {noun}s"),
        };

        match (self.collections.len(), self.environments.len()) {
            (collections, 0) => format!("Imported {}.", count(collections, "collection")),
            (0, environments) => format!("Imported {}.", count(environments, "environment")),
            (collections, environments) => format!(
                "Imported {} and {}.",
                count(collections, "collection"),
                count(environments, "environment")
            ),
        }
    }
}

/// Converts pasted text, explaining failures as ones of text, not of a file.
fn read_text(source: &str) -> Result<Import, String> {
    import::parse(source).map_err(|error| match error {
        ImportError::Syntax(error) => format!("The text is not valid JSON or YAML: {error}"),
        ImportError::UnknownFormat => "The text is not a cURL command, a Postman collection or \
                                       environment, or an OpenAPI specification."
            .to_owned(),
        error => error.to_string(),
    })
}

/// Writes a converted collection or environment.
fn save(
    import: Import,
    directory: Option<&Path>,
    environments: &GlobalEnvironments,
    outcome: &mut Outcome,
) -> Result<(), String> {
    match import {
        Import::Collection(import) => {
            let directory = directory.ok_or("No collections directory is configured.")?;
            let collection = import
                .collection
                .write(directory)
                .map_err(|error| format!("Could not import the collection: {error}"))?;

            let name = path_name(&collection.path);
            outcome.skipped.extend(
                import
                    .skipped
                    .into_iter()
                    .map(|request| (name.clone(), request)),
            );
            outcome.collections.push(collection);
        }
        Import::Environment(environment) => {
            let name = environments
                .import(&environment.name, environment.variables)
                .map_err(|error| format!("Could not import the environment: {error}"))?;
            outcome.environments.push(name);
        }
    }

    Ok(())
}

impl Render for ImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(summary) = &self.summary {
            return self.render_summary(summary, cx);
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
                        this.import(Source::Paths(paths.paths().to_vec()), window, cx);
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
                                            "Drop files or folders to import"
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
                                                    .label("files or folders")
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
                    .child("Postman environments")
                    .child("Postman workspace and collection folders, with gRPC requests")
                    .child("OpenAPI 3 and Swagger 2.0, in JSON or YAML"),
            )
    }
}
