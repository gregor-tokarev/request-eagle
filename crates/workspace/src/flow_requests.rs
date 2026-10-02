use std::path::Path;

use collection::{Collection, Entry};
use collections_panel_ui::CollectionPanel;
use gpui_kit::{App, Entity, SharedString};
use request::Request;
use tab_ui::{FlowRequest, FlowRequests};

/// The HTTP requests saved in the collections sidebar, which flows send.
pub(crate) struct SidebarRequests(pub(crate) Entity<CollectionPanel>);

impl FlowRequests for SidebarRequests {
    fn all(&self, cx: &App) -> Vec<FlowRequest> {
        let mut requests = Vec::new();

        for collection in self.0.read(cx).registry().collections() {
            let name = folder_name(&collection.path);
            collect(collection, &collection.entries, &name, &mut requests);
        }

        requests
    }

    fn find(&self, id: &str, cx: &App) -> Option<FlowRequest> {
        let (collection, file) = self.0.read(cx).registry().request_by_id(id)?;
        let Request::Http(request) = &file.request else {
            return None;
        };

        // The folders between the collection and the request.
        let mut location = folder_name(&collection.path);
        if let Ok(relative) = file.path.strip_prefix(&collection.path) {
            for folder in relative.parent().into_iter().flat_map(Path::iter) {
                location.push_str(" › ");
                location.push_str(&folder.to_string_lossy());
            }
        }

        Some(FlowRequest {
            id: file.id.clone(),
            name: file.name.clone().into(),
            location: location.into(),
            collection: collection.path.clone(),
            request: request.clone(),
            collection_auth: collection.auth().clone(),
        })
    }
}

fn collect(
    collection: &Collection,
    entries: &[Entry],
    location: &str,
    requests: &mut Vec<FlowRequest>,
) {
    for entry in entries {
        match entry {
            Entry::File(file) => {
                if let Request::Http(request) = &file.request {
                    requests.push(FlowRequest {
                        id: file.id.clone(),
                        name: file.name.clone().into(),
                        location: SharedString::from(location.to_owned()),
                        collection: collection.path.clone(),
                        request: request.clone(),
                        collection_auth: collection.auth().clone(),
                    });
                }
            }
            Entry::Directory(folder) => {
                let location = format!("{location} › {}", folder.name);
                collect(collection, &folder.entries, &location, requests);
            }
            Entry::Flow(_) => {}
        }
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
