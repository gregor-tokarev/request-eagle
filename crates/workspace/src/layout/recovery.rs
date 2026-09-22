use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::*;

use super::{
    main_view::{MainView, PageTab},
    request_draft::RequestDraft,
};
use crate::session::{
    RecoveredTab, SessionCheckpoint, SessionSnapshot, SessionStore, SessionWriter,
};

const SAVE_ERROR: &str = "Could not save session recovery";

pub(super) struct Recovery {
    writer: SessionWriter,
    cached: HashMap<u64, Arc<RecoveredTab>>,
    order: Vec<u64>,
    selected: Option<usize>,
    revision: u64,
    _quit: Subscription,
}

impl Recovery {
    fn capture(&mut self, tabs: &[PageTab], selected: Option<usize>, cx: &App) -> bool {
        let mut changed = self.revision == 0;
        let mut cached = HashMap::new();
        let mut order = Vec::new();
        let mut recovered_selected = None;

        for (index, tab) in tabs.iter().enumerate() {
            let Ok(draft) = tab.page.clone().downcast::<RequestDraft>() else {
                continue;
            };
            let draft = draft.read(cx);

            if selected == Some(index) {
                recovered_selected = Some(order.len());
            }

            let recovered = match self.cached.remove(&tab.id) {
                Some(previous) if matches_draft(&previous, tab, draft) => previous,
                _ => {
                    changed = true;
                    Arc::new(recover_tab(tab, draft))
                }
            };
            cached.insert(tab.id, recovered);
            order.push(tab.id);
        }

        changed |= self.order != order || self.selected != recovered_selected;
        self.cached = cached;
        self.order = order;
        self.selected = recovered_selected;

        changed
    }

    fn checkpoint(&self) -> SessionCheckpoint {
        SessionCheckpoint {
            tabs: self
                .order
                .iter()
                .map(|id| self.cached[id].clone())
                .collect(),
            selected: self.selected,
        }
    }
}

fn matches_draft(recovered: &RecoveredTab, tab: &PageTab, draft: &RequestDraft) -> bool {
    recovered.title == tab.title.as_ref()
        && recovered.name == draft.name.as_ref()
        && recovered.collection.as_deref() == draft.collection.as_deref()
        && recovered.request_path == tab.request_path
        && recovered.environment_path == draft.environment_path
        && recovered.request == draft.request
        && recovered.saved_request.as_ref() == Some(&draft.saved_request)
}

fn recover_tab(tab: &PageTab, draft: &RequestDraft) -> RecoveredTab {
    RecoveredTab {
        title: tab.title.to_string(),
        name: draft.name.to_string(),
        collection: draft.collection.as_ref().map(ToString::to_string),
        request_path: tab.request_path.clone(),
        environment_path: draft.environment_path.clone(),
        request: draft.request.clone(),
        saved_request: Some(draft.saved_request.clone()),
    }
}

/// Explicit overrides keep fixture and development sessions separate from personal work.
pub(crate) fn state_directory() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("REQUEST_EAGLE_STATE_DIR") {
        return Some(PathBuf::from(directory));
    }

    if let Some(directory) = std::env::var_os("REQUEST_EAGLE_COLLECTIONS_DIR") {
        return collection_state_directory(Path::new(&directory));
    }

    dirs::home_dir().map(|home| home.join(".request-eagle"))
}

pub(super) fn collection_state_directory(collections: &Path) -> Option<PathBuf> {
    let mut name = OsString::from(".");
    name.push(collections.file_name()?);
    name.push(".request-eagle-state");

    // CollectionRegistry treats child directories as collections, so keep
    // fixture state beside the root rather than creating another collection.
    Some(collections.with_file_name(name))
}

impl MainView {
    pub(crate) fn enable_workflow_storage(&mut self, directory: PathBuf, cx: &mut Context<Self>) {
        match crate::history::History::load(directory.join("history.json")) {
            Ok(history) => self.history = history,
            Err(error) => self.storage_error = Some(format!("Could not load history: {error}")),
        }

        let store = SessionStore::new(directory.join("session.json"));
        match store.load() {
            Ok(snapshot) => {
                if let Some(snapshot) = snapshot {
                    self.restore_session(snapshot, cx);
                }

                let quit = cx.on_app_quit(|this, cx| {
                    // Include edits whose observer has not run yet. The writer
                    // prevents older work from replacing this final snapshot.
                    this.flush_session(cx);
                    async {}
                });
                self.recovery = Some(Recovery {
                    writer: SessionWriter::new(store),
                    cached: HashMap::new(),
                    order: Vec::new(),
                    selected: None,
                    revision: 0,
                    _quit: quit,
                });
                self.save_session(cx);
            }
            Err(error) => {
                self.storage_error = Some(format!(
                    "Could not recover the previous session: {error}. Its file was left unchanged; recovery saving is disabled."
                ));
            }
        }
    }

    pub(super) fn restore_session(&mut self, snapshot: SessionSnapshot, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.selected = None;

        for tab in snapshot.tabs {
            let mut draft = RequestDraft::new();
            draft.name = tab.name.into();
            draft.collection = tab.collection.map(Into::into);
            draft.saved_request = tab.saved_request.unwrap_or_else(|| tab.request.clone());
            draft.request = tab.request;
            draft.environment_path = tab.environment_path;
            let index = self.open_draft(tab.title.into(), draft, cx);
            self.tabs[index].request_path = tab.request_path;
        }

        self.selected = snapshot.selected;
        self.scroll_to_tab = snapshot.selected;
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn session_snapshot(&self, cx: &App) -> SessionSnapshot {
        let mut selected = None;
        let mut tabs = Vec::new();

        for (index, tab) in self.tabs.iter().enumerate() {
            let Ok(draft) = tab.page.clone().downcast::<RequestDraft>() else {
                continue;
            };

            if self.selected == Some(index) {
                selected = Some(tabs.len());
            }

            tabs.push(recover_tab(tab, draft.read(cx)));
        }

        SessionSnapshot { tabs, selected }
    }

    pub(super) fn recover_draft_change(
        &mut self,
        page: &Entity<RequestDraft>,
        cx: &mut Context<Self>,
    ) {
        let Some(recovery) = &self.recovery else {
            return;
        };
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.page.entity_id() == page.entity_id())
        else {
            return;
        };

        // Responses, layout, and focus notify this entity too. Compare the one
        // affected draft before cloning data or scheduling a disk write.
        if recovery
            .cached
            .get(&tab.id)
            .is_some_and(|cached| matches_draft(cached, tab, page.read(cx)))
        {
            return;
        }

        self.save_session(cx);
    }

    pub(super) fn save_session(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = &mut self.recovery else {
            return;
        };

        if !recovery.capture(&self.tabs, self.selected, cx) {
            return;
        }

        recovery.revision += 1;
        let revision = recovery.revision;
        let write = recovery.writer.checkpoint(recovery.checkpoint());
        let task = cx.background_executor().spawn(async move { write.write() });

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .recovery
                    .as_ref()
                    .is_none_or(|recovery| recovery.revision != revision)
                {
                    return;
                }

                match result {
                    Ok(()) => {
                        if this
                            .storage_error
                            .as_deref()
                            .is_some_and(|error| error.starts_with(SAVE_ERROR))
                        {
                            this.storage_error = None;
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        this.storage_error = Some(format!("{SAVE_ERROR}: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn flush_session(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = &mut self.recovery else {
            return;
        };
        recovery.capture(&self.tabs, self.selected, cx);
        recovery.revision += 1;

        if let Err(error) = recovery.writer.flush(recovery.checkpoint()) {
            self.storage_error = Some(format!("{SAVE_ERROR}: {error}"));
        }
    }

    #[cfg(test)]
    pub(super) fn recovery_revision(&self) -> Option<u64> {
        self.recovery.as_ref().map(|recovery| recovery.revision)
    }
}
