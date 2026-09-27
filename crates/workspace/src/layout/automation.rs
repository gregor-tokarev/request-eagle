use super::main_view::MainView;
use gpui_kit::*;
use request_eagle_automation::{Command, MAX_BODY_CHUNK, RequestInput};
use serde_json::{Value, json};
use tab_ui::RequestDraft;

impl MainView {
    pub(crate) fn automation_tabs(&self, cx: &App) -> Result<Value, String> {
        if self.tabs.iter().any(|tab| {
            tab.request_path
                .as_ref()
                .is_some_and(|path| path.to_str().is_none())
        }) {
            return Err("Tab request paths must be valid UTF-8 for CLI commands".into());
        }

        Ok(json!(
            self.tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| json!({
                    "id": tab.id, "title": tab.title.as_ref(), "path": tab.request_path,
                    "request_id": tab.request_id.as_ref().map(|id| id.as_ref()),
                    "selected": self.selected == Some(index), "dirty": tab.page.state(cx).dirty,
                }))
                .collect::<Vec<_>>()
        ))
    }

    pub(crate) fn automation_draft(&self, id: u64) -> Result<Entity<RequestDraft>, String> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("Unknown tab ID")?;
        tab.page
            .view()
            .downcast::<RequestDraft>()
            .map_err(|_| "This tab is not a request draft".into())
    }

    pub(crate) fn automation_saved_target(
        &self,
        id: u64,
    ) -> Result<Option<(std::path::PathBuf, SharedString)>, String> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("Unknown tab ID")?;
        if tab
            .request_path
            .as_ref()
            .is_some_and(|path| path.to_str().is_none())
        {
            return Err("Saved request paths must be valid UTF-8 for CLI commands".into());
        }

        Ok(tab.request_path.clone().zip(tab.request_id.clone()))
    }

    pub(crate) fn automation_tab_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if matches!(command, Command::TabsNew {}) {
            self.new_tab(cx);
            self.prepare_active_tab(window, cx);
            return Ok(json!({"tab": self.tabs[self.selected.unwrap()].id}));
        }
        if matches!(command, Command::TabsList {}) {
            return self.automation_tabs(cx);
        }
        let id = match &command {
            Command::TabsSelect { tab }
            | Command::TabsClose { tab, .. }
            | Command::DraftsGet { tab }
            | Command::DraftsSet { tab, .. }
            | Command::RequestsSend { tab, .. }
            | Command::RequestsCancel { tab }
            | Command::ResponsesGet { tab, .. } => *tab,
            _ => return Err("Unsupported tab command".into()),
        };
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or("Unknown tab ID")?;
        match command {
            Command::TabsSelect { .. } => {
                self.select_tab(index, cx);
                self.prepare_active_tab(window, cx);
            }
            Command::TabsClose { discard, .. } => {
                if self.tabs[index].page.state(cx).dirty && !discard {
                    return Err("Unsaved draft: save it or set discard=true".into());
                }
                self.remove_tab(index, cx);
                self.prepare_active_tab(window, cx);
            }
            command => {
                let draft = self.automation_draft(id)?;
                match command {
                    Command::DraftsGet { .. } => {
                        return Ok(
                            json!({"tab": id, "request": RequestInput::from(&draft.read(cx).request), "dirty": draft.read(cx).is_dirty(), "sending": draft.read(cx).is_sending()}),
                        );
                    }
                    Command::DraftsSet { request, .. } => draft.update(cx, |draft, cx| {
                        draft.replace_request(request.into(), window, cx)
                    }),
                    Command::RequestsSend { trust_scripts, .. } => draft
                        .update(cx, |draft, cx| {
                            draft.send_from_automation(trust_scripts, window, cx)
                        })?,
                    Command::RequestsCancel { .. } => {
                        draft.update(cx, |draft, cx| draft.cancel(cx))
                    }
                    Command::ResponsesGet { offset, limit, .. } => {
                        if limit == 0 || limit > MAX_BODY_CHUNK {
                            return Err(format!("limit must be between 1 and {MAX_BODY_CHUNK}"));
                        }
                        return draft.read(cx).automation_response(offset, limit, cx);
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(json!({"tab": id}))
    }
}
