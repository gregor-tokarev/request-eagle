use std::path::{Path, PathBuf};

use collection::{CollectionEditError, FileEntry, Request};
use gpui_kit::{Context, SharedString, Window};

use super::CollectionPanel;

#[derive(Clone)]
pub struct SaveDestination {
    pub path: PathBuf,
    pub label: SharedString,
    pub collection: SharedString,
    pub folders: Vec<SharedString>,
}

impl CollectionPanel {
    pub fn save_destinations(&self) -> Vec<SaveDestination> {
        self.tree
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if !item.is_branch() {
                    return None;
                }
                let mut labels = vec![item.label.clone()];
                let mut parent = self.tree.items[index].parent;
                while let Some(index) = parent {
                    labels.push(self.tree.items[index].label.clone());
                    parent = self.tree.items[index].parent;
                }
                labels.reverse();
                Some(SaveDestination {
                    path: item.path.clone(),
                    label: labels
                        .iter()
                        .map(|label| label.as_ref())
                        .collect::<Vec<_>>()
                        .join(" › ")
                        .into(),
                    collection: labels[0].clone(),
                    folders: labels[1..].to_vec(),
                })
            })
            .collect()
    }

    pub fn create_save_collection(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, CollectionEditError> {
        let path = self.collections.create_collection()?;
        self.rebuild_tree(Some(&path), None, cx);
        Ok(path)
    }

    pub fn save_new_request(
        &mut self,
        parent: &Path,
        name: &str,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<FileEntry, CollectionEditError> {
        let path = self
            .collections
            .create_request_with(parent, name, request)?;
        let file = self
            .collections
            .file(&path)
            .expect("created request exists")
            .clone();
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.collapsed
            .retain(|&index| !path.starts_with(&self.tree.items[index].path));
        self.rebuild_tree(Some(&path), None, cx);
        Ok(file)
    }
}
