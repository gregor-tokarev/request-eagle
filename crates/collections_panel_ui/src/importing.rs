use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use collection::Collection;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{panel::CollectionPanel, tree::path_name};

impl CollectionPanel {
    /// Opens a dialog that imports a Postman collection or an OpenAPI
    /// specification as a new collection.
    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = cx.entity().downgrade();
        let dialog = cx.new(|_| ImportDialog {
            panel,
            importing: false,
            error: None,
            skipped: None,
            _task: None,
        });

        window.open_dialog(cx, move |modal, window, _| {
            modal
                .title("Import")
                .w(rems(30.).to_pixels(window.rem_size()))
                .child(dialog.clone())
        });
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

/// Imports a Postman collection or an OpenAPI specification from a file as a
/// new collection.
struct ImportDialog {
    panel: WeakEntity<CollectionPanel>,
    importing: bool,
    error: Option<String>,
    /// The imported collection's name and the requests it left out, shown
    /// until the dialog is dismissed.
    skipped: Option<(String, Vec<String>)>,
    _task: Option<Task<()>>,
}

impl ImportDialog {
    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
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
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
            let import = import::parse(&source).map_err(|error| error.to_string())?;
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
            1 => "1 request uses an HTTP method Request Eagle cannot send, so it was left out:"
                .to_owned(),
            count => format!(
                "{count} requests use HTTP methods Request Eagle cannot send, so they were left out:"
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
        let format = |name: &'static str, details: &'static str| {
            h_flex()
                .gap_2()
                .child(name)
                .child(div().text_color(theme.muted_foreground).child(details))
        };

        v_flex()
            .debug_selector(|| "import-dialog".into())
            .gap_4()
            .child(
                v_flex()
                    .id("import-drop-zone")
                    .items_center()
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
                        Icon::default()
                            .path("icons/import.svg")
                            .size_6()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Drop a file here to import it as a new collection"),
                    )
                    .child(
                        Button::new("choose-import-file")
                            .debug_selector(|| "choose-import-file".into())
                            .primary()
                            .label(if self.importing {
                                "Importing…"
                            } else {
                                "Choose File…"
                            })
                            .loading(self.importing)
                            .disabled(self.importing)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_file(window, cx)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child("Supported formats"),
                    )
                    .child(format("Postman Collection", "v2.0 and v2.1, JSON"))
                    .child(format("OpenAPI", "3.x and Swagger 2.0, JSON or YAML")),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .debug_selector(|| "import-error".into())
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
    }
}
