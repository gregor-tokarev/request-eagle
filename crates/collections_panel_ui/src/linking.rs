use std::path::PathBuf;

use collection::{LinkedFolder, directory_name};
use gpui_kit::*;

use super::panel::CollectionPanel;

impl CollectionPanel {
    /// Asks for folders to show as collections where they are, such as in a
    /// project's Git repository, and adds them. Each folder must be empty or
    /// hold a collection.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open as Collection".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let paths = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(format!("Could not open the folder picker: {error}"));
                        cx.notify();
                    });
                    return;
                }
                // The picker was cancelled.
                _ => return,
            };

            // A large folder takes a while to read.
            let folders: Vec<(PathBuf, _)> = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let folder = LinkedFolder::read(&path);
                            (path, folder)
                        })
                        .collect()
                })
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.link_folders(folders, window, cx)
            });
        })
        .detach();
    }

    fn link_folders(
        &mut self,
        folders: Vec<(
            PathBuf,
            Result<LinkedFolder, collection::CollectionEditError>,
        )>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut errors = Vec::new();
        let mut linked = None;

        for (path, folder) in folders {
            let result = folder.and_then(|folder| {
                self.collections
                    .update(cx, |collections, cx| collections.link(folder, cx))
            });

            match result {
                Ok(path) => linked = Some(path),
                Err(error) => errors.push(format!(
                    "Could not open {} as a collection: {error}",
                    directory_name(&path)
                )),
            }
        }

        if linked.is_some() {
            analytics::capture("collection_folder_opened", serde_json::json!({}), cx);
        }

        self.rename = None;
        self.pending_delete = None;
        self.error = (!errors.is_empty()).then(|| errors.join("\n"));
        if let Some(path) = linked {
            self.reveal(&path, None, window, cx);
            window.focus(&self.focus, cx);
        }

        cx.notify();
    }
}
