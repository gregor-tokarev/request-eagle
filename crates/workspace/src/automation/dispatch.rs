use crate::{layout::main_view::RequestSaveRequested, workspace::Layout};
use gpui_kit::*;
use request_eagle_automation::{Command, Page};
use serde_json::{Value, json};

impl Layout {
    pub(crate) fn automation_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        match command {
            Command::AppStatus {} => Ok(
                json!({"pid": std::process::id(), "title": std::env::var("REQUEST_EAGLE_WINDOW_TITLE").unwrap_or_else(|_| "Request Eagle".into()), "version": self.updater.read(cx).current_version(), "settings_visible": self.settings_visible}),
            ),
            Command::EntriesDelete { confirm: false, .. } => {
                Err("Set confirm=true to permanently delete this saved entry".into())
            }
            command @ (Command::CollectionsList { .. }
            | Command::CollectionsCreate {}
            | Command::FoldersCreate { .. }
            | Command::RequestsCreate { .. }
            | Command::RequestsGet { .. }
            | Command::EntriesRename { .. }
            | Command::EntriesMove { .. }
            | Command::EntriesDelete { .. }) => self.sidebar.update(cx, |sidebar, cx| {
                sidebar.automation_command(command, window, cx)
            }),
            Command::RequestsOpen { path } => {
                let sidebar = self.sidebar.read(cx);
                let file = sidebar
                    .registry()
                    .file(&path)
                    .ok_or("Unknown saved request path")?
                    .clone();
                let destination = sidebar
                    .save_destinations()
                    .into_iter()
                    .find(|d| Some(d.path.as_path()) == path.parent())
                    .ok_or("Unknown request parent")?;
                self.main_view.update(cx, |view, cx| {
                    view.open_request(
                        &path,
                        file.id.into(),
                        file.name.into(),
                        destination.collection,
                        destination.folders,
                        &file.request,
                        cx,
                    );
                    view.prepare_active_tab(window, cx);
                    view.automation_tabs(cx)
                })
            }
            Command::DraftsSave { tab, parent, name } => {
                let view = self.main_view.read(cx);
                let draft = view.automation_draft(tab)?;
                let request = draft.read(cx).request.clone();
                if let Some(parent) = parent {
                    let name = name.ok_or("Supply name with parent")?;
                    let destination = self
                        .sidebar
                        .read(cx)
                        .save_destinations()
                        .into_iter()
                        .find(|d| d.path == parent)
                        .ok_or("Unknown save destination")?;
                    let file = self
                        .sidebar
                        .update(cx, |sidebar, cx| {
                            sidebar.save_new_request(&parent, &name, request.into(), window, cx)
                        })
                        .map_err(|e| e.to_string())?;
                    self.main_view.update(cx, |view, cx| {
                        view.attach_saved_request(tab, &file, &destination, window, cx)
                    });
                    Ok(json!({"tab": tab, "path": file.path, "id": file.id}))
                } else {
                    if name.is_some() {
                        return Err("name requires parent".into());
                    }
                    let (path, request_id) = view
                        .automation_saved_target(tab)?
                        .ok_or("New drafts need parent and name")?;
                    self.sidebar
                        .update(cx, |sidebar, cx| {
                            sidebar.save_request(&path, &request_id, request.clone().into(), cx)
                        })
                        .map_err(|e| e.to_string())?;
                    let event = RequestSaveRequested {
                        tab_id: tab,
                        path: path.clone(),
                        request_id,
                        request,
                    };
                    self.main_view
                        .update(cx, |view, cx| view.finish_save(&event, Ok(()), window, cx));
                    Ok(json!({"tab": tab, "path": path}))
                }
            }
            command @ (Command::TabsList {}
            | Command::TabsNew {}
            | Command::TabsSelect { .. }
            | Command::TabsClose { .. }
            | Command::DraftsGet { .. }
            | Command::DraftsSet { .. }
            | Command::RequestsSend { .. }
            | Command::RequestsCancel { .. }
            | Command::ResponsesGet { .. }) => self.main_view.update(cx, |view, cx| {
                view.automation_tab_command(command, window, cx)
            }),
            Command::UiShow { page } => {
                if matches!(page, Page::Workspace) {
                    self.close_settings(window, cx);
                } else {
                    self.open_settings(window, cx);
                    let page = match page {
                        Page::General => settings_ui::SettingsPage::General,
                        Page::Appearance => settings_ui::SettingsPage::Appearance,
                        Page::Proxy => settings_ui::SettingsPage::Proxy,
                        Page::Keybindings => settings_ui::SettingsPage::Keybindings,
                        Page::Workspace => unreachable!(),
                    };
                    self.settings
                        .update(cx, |settings, cx| settings.select_page(page, window, cx));
                }
                Ok(json!({}))
            }
            Command::UiSidebar { visible } => {
                self.sidebar_visible.update(cx, |state, cx| {
                    *state = visible;
                    cx.notify();
                });
                Ok(json!({"visible": visible}))
            }
            Command::UpdatesStatus {} => Ok(self.automation_update_status(cx)),
            Command::UpdatesCheck {} => {
                self.updater.update(cx, |updater, cx| updater.check(cx));
                Ok(self.automation_update_status(cx))
            }
            Command::UpdatesDownload {} => {
                if !matches!(
                    self.updater.read(cx).status(),
                    updater::UpdateStatus::Available(_)
                ) {
                    return Err("No update is available; check updates.status".into());
                }
                self.updater.update(cx, |updater, cx| updater.download(cx));
                Ok(self.automation_update_status(cx))
            }
            Command::UpdatesInstall { confirm } => {
                if !confirm {
                    return Err("Set confirm=true to quit and install the update".into());
                }
                if !matches!(
                    self.updater.read(cx).status(),
                    updater::UpdateStatus::Ready(_)
                ) {
                    return Err("No verified update is ready".into());
                }
                // Allow the command reply to be delivered before quitting.
                let updater = self.updater.clone();
                cx.spawn(async move |_, cx| {
                    smol::Timer::after(std::time::Duration::from_millis(200)).await;
                    updater.update(cx, |updater, cx| updater.relaunch(cx));
                })
                .detach();
                Ok(json!({"relaunching": true}))
            }
            command => {
                let page = match &command {
                    Command::SettingsRequest { .. } => Some(settings_ui::SettingsPage::General),
                    Command::SettingsAppearance { .. } => {
                        Some(settings_ui::SettingsPage::Appearance)
                    }
                    _ => None,
                };
                let result = super::settings::apply(command, cx)?;
                if let Some(page) = page {
                    self.settings.update(cx, |settings, cx| {
                        settings.refresh_preferences(page, window, cx)
                    });
                }
                Ok(result)
            }
        }
    }

    fn automation_update_status(&self, cx: &App) -> Value {
        use updater::UpdateStatus;
        let (state, details) = match self.updater.read(cx).status() {
            UpdateStatus::Idle => ("idle", json!({})),
            UpdateStatus::Checking => ("checking", json!({})),
            UpdateStatus::UpToDate => ("up_to_date", json!({})),
            UpdateStatus::Available(manifest) => {
                ("available", json!({"version": manifest.version}))
            }
            UpdateStatus::Downloading {
                version,
                downloaded_bytes,
                total_bytes,
            } => (
                "downloading",
                json!({"version": version, "downloaded_bytes": downloaded_bytes, "total_bytes": total_bytes}),
            ),
            UpdateStatus::Verifying(version) => ("verifying", json!({"version": version})),
            UpdateStatus::Ready(version) => ("ready", json!({"version": version})),
            UpdateStatus::Error(error) => ("error", json!({"message": error})),
        };
        json!({"state": state, "details": details})
    }
}
