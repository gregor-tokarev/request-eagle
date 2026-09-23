use gpui_kit::component::{
    button::*,
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{main_view::MainView, request_draft::RequestDraft};

pub(crate) struct ImportRequested {
    pub name: String,
    pub requests: Vec<crate::imports::ImportedRequest>,
}

impl EventEmitter<ImportRequested> for MainView {}

impl MainView {
    pub(crate) fn relocate_history_environment(
        &mut self,
        previous_path: &std::path::Path,
        environment_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        if self
            .history
            .relocate_environment(previous_path, environment_path)
        {
            self.save_history(cx);
        }
        for tab in &self.tabs {
            let Ok(draft) = tab.page.clone().downcast::<RequestDraft>() else {
                continue;
            };
            let should_rebind = draft
                .read(cx)
                .environment_path
                .as_ref()
                .and_then(|path| path.parent())
                .is_some_and(|root| root == previous_path);
            if should_rebind {
                draft.update(cx, |draft, cx| {
                    draft.environment_path = Some(environment_path.to_path_buf());
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    pub(super) fn workflow_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let file_view = view.clone();

        h_flex().gap_1().flex_none()
            .child(Button::new("import-request").debug_selector(|| "import-request".into()).ghost().small().label("Import").dropdown_menu(move |menu, _, _| {
                let view = view.clone();
                let file_view = file_view.clone();
                menu.item(PopupMenuItem::new("Paste cURL / Postman").on_click(move |_, window, cx| {
                    let text = cx.read_from_clipboard().and_then(|item| item.text());
                    let _ = view.update(cx, |view, cx| {
                        match text {
                            Some(text) => view.import_text(&text, window, cx),
                            None => { view.workflow_error = Some("Copy a cURL command or Postman collection JSON, then choose Paste cURL / Postman.".into()); cx.notify(); }
                        }
                    });
                })).item(PopupMenuItem::new("Choose import file…").on_click(move |_, window, cx| {
                    let _ = file_view.update(cx, |view, cx| view.import_file(window, cx));
                }))
            }))
            .child(Button::new("request-history").debug_selector(|| "request-history".into()).ghost().small().label("History").on_click(cx.listener(|this, _, _, cx| {
                this.history_visible = !this.history_visible;
                cx.notify();
            })))
    }

    pub(super) fn import_text(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match crate::imports::parse_import(source) {
            Ok(requests) => {
                self.workflow_error = None;
                let name = serde_json::from_str::<serde_json::Value>(source)
                    .ok()
                    .and_then(|json| json.get("info")?.get("name")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Imported cURL".into());
                cx.emit(ImportRequested { name, requests });
                self.focus(window, cx);
            }
            Err(error) => self.workflow_error = Some(format!("Import failed: {error}")),
        }

        cx.notify();
    }

    pub(crate) fn imported_requests(
        &mut self,
        result: Result<Vec<collection::ImportedFile>, collection::CollectionEditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(files) => {
                let recovery = self.recovery.take();
                for file in files {
                    if !self.collection_paths.contains(&file.collection_path) {
                        self.collection_paths.push(file.collection_path.clone());
                    }
                    let collection = file
                        .collection_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.open_request(
                        &file.path,
                        file.id.into(),
                        file.name.into(),
                        collection.into(),
                        file.path
                            .strip_prefix(&file.collection_path)
                            .ok()
                            .and_then(std::path::Path::parent)
                            .map(|path| {
                                path.iter()
                                    .map(|part| part.to_string_lossy().into_owned().into())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        &file.request.into(),
                        cx,
                    );
                }
                self.recovery = recovery;
                self.save_session(cx);
                self.focus(window, cx);
            }
            Err(error) => {
                self.workflow_error = Some(format!("Could not save imported collection: {error}"))
            }
        }
        cx.notify();
    }

    fn import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Postman JSON or cURL text".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => return,
                result => {
                    let _ = this.update(cx, |this, cx| {
                        this.workflow_error =
                            Some(format!("Could not choose import file: {result:?}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else { return };
            let text = cx
                .background_executor()
                .spawn(async move {
                    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
                    if metadata.len() > 16 * 1024 * 1024 {
                        return Err("Import files must be 16 MiB or smaller.".to_string());
                    }
                    std::fs::read_to_string(path).map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match text {
                Ok(text) => this.import_text(&text, window, cx),
                Err(error) => {
                    this.workflow_error = Some(format!("Could not read import file: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn history_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .debug_selector(|| "history-panel".into())
            .max_h(px(240.))
            .border_b_1()
            .border_color(cx.theme().border)
            .px_3()
            .py_2()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child("Recent requests"))
                    .child(
                        Button::new("clear-history")
                            .debug_selector(|| "clear-history".into())
                            .ghost()
                            .small()
                            .label(if self.history.is_clearing() {
                                "Clearing history…"
                            } else {
                                "Clear history"
                            })
                            .disabled(self.history.is_clearing())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_history(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Last 100 attempts, stored locally. Select one to reopen an editable copy.",
                    ),
            )
            .child(
                v_flex()
                    .id("history-list")
                    .overflow_y_scroll()
                    .min_h_0()
                    .children(
                        self.history
                            .entries
                            .iter()
                            .enumerate()
                            .map(|(index, entry)| {
                                let entry = entry.clone();
                                let label = format!(
                                    "{} {}",
                                    entry.request.method.as_str(),
                                    entry.request.path
                                );
                                Button::new(("history-entry", index))
                                    .debug_selector(move || format!("history-entry-{index}"))
                                    .ghost()
                                    .small()
                                    .w_full()
                                    .justify_start()
                                    .label(label)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let mut draft = RequestDraft::new();
                                        draft.name = entry.name.clone().into();
                                        draft.request = (*entry.request).clone();
                                        draft.environment_path = entry.environment_path.clone();
                                        this.open_draft(entry.name.clone().into(), draft, cx);
                                        this.history_visible = false;
                                        this.focus(window, cx);
                                    }))
                            }),
                    ),
            )
            .when(self.history.entries.is_empty(), |view| {
                view.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("No requests sent yet."),
                )
            })
    }
}
