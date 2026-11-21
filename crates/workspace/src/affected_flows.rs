use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use collection::{CollectionRegistry, Entry, FileEntry};
use collections_panel_ui::{AffectedFlow, FlowImpact};
use flow::{Flow, SavedFlow};
use request::Request;

/// A flow tab's live state, which may include unsaved edits over its file.
pub(crate) struct FlowSnapshot<'a> {
    pub path: &'a Path,
    pub id: &'a str,
    pub name: &'a str,
    pub flow: &'a Flow,
}

/// Flows that reference a saved HTTP request at or under `path`, in the same
/// case-insensitive name order as the Flows sidebar.
///
/// Both the saved flow on disk and any open tab draft are checked so neither an
/// unsaved addition nor an unsaved removal hides a real dependency. Each flow
/// is listed at most once.
pub(crate) fn find(
    registry: &CollectionRegistry,
    path: &Path,
    saved_flows: &[SavedFlow],
    open_flows: &[FlowSnapshot<'_>],
) -> Vec<AffectedFlow> {
    let removed = removed_http_requests(registry, path);
    if removed.is_empty() {
        return Vec::new();
    }

    let open_by_key: HashMap<(&Path, &str), &FlowSnapshot<'_>> = open_flows
        .iter()
        .map(|snapshot| ((snapshot.path, snapshot.id), snapshot))
        .collect();
    let mut seen_open = HashSet::new();
    let mut affected = Vec::new();

    for saved in saved_flows {
        let key = (saved.path.as_path(), saved.id.as_str());
        let saved_impact = flow_impact(&saved.flow, &removed);
        let (name, open_impact) = match open_by_key.get(&key) {
            Some(open) => {
                seen_open.insert(key);
                (open.name, flow_impact(open.flow, &removed))
            }
            None => (saved.name.as_str(), None),
        };

        if let Some(impact) = combine_impact(saved_impact, open_impact) {
            affected.push(AffectedFlow {
                path: saved.path.clone(),
                id: saved.id.clone().into(),
                name: name.to_owned().into(),
                impact,
            });
        }
    }

    for open in open_flows {
        let key = (open.path, open.id);
        if seen_open.insert(key)
            && let Some(impact) = flow_impact(open.flow, &removed)
        {
            affected.push(AffectedFlow {
                path: open.path.to_path_buf(),
                id: open.id.to_owned().into(),
                name: open.name.to_owned().into(),
                impact,
            });
        }
    }

    affected.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.path, &a.id).cmp(&(b.name.to_lowercase(), &b.path, &b.id))
    });

    affected
}

/// Saved HTTP request IDs whose resolution changes or disappears when `path`
/// is deleted or unlinked from `registry`, paired with the resulting impact.
fn removed_http_requests<'a>(
    registry: &'a CollectionRegistry,
    path: &Path,
) -> HashMap<&'a str, FlowImpact> {
    let mut files = Vec::new();
    for collection in registry.collections() {
        if collection.path == path {
            collect_files(&collection.entries, &mut files);
            break;
        }
        if path.starts_with(&collection.path)
            && collect_files_at(&collection.entries, path, &mut files)
        {
            break;
        }
    }

    files
        .into_iter()
        .filter(|file| matches!(file.request, Request::Http(_)))
        .filter(|file| {
            // Flows resolve requests with `request_by_id`, which returns the
            // first matching file. Deleting a shadowed duplicate leaves flows
            // pointing at the same active request.
            registry
                .request_by_id(&file.id)
                .is_some_and(|(_, resolved)| resolved.path == file.path)
        })
        .map(|file| {
            let impact = if has_fallback_http_request(registry, path, &file.id) {
                FlowImpact::DifferentRequest
            } else {
                FlowImpact::MissingRequest
            };
            (file.id.as_str(), impact)
        })
        .collect()
}

/// Whether removing `deleted_path` leaves another saved file with `id` that
/// `CollectionRegistry::request_by_id` will resolve as an HTTP request.
fn has_fallback_http_request(registry: &CollectionRegistry, deleted_path: &Path, id: &str) -> bool {
    let next = registry
        .collections()
        .iter()
        .filter(|collection| collection.path != deleted_path)
        .find_map(|collection| find_by_id_excluding(&collection.entries, deleted_path, id));

    next.is_some_and(|file| matches!(file.request, Request::Http(_)))
}

fn find_by_id_excluding<'a>(
    entries: &'a [Entry],
    deleted_path: &Path,
    id: &str,
) -> Option<&'a FileEntry> {
    entries.iter().find_map(|entry| match entry {
        Entry::File(file) if file.path == deleted_path => None,
        Entry::File(file) if file.id == id => Some(file),
        Entry::Directory(folder) if folder.path == deleted_path => None,
        Entry::Directory(folder) => find_by_id_excluding(&folder.entries, deleted_path, id),
        _ => None,
    })
}

fn flow_impact(flow: &Flow, removed: &HashMap<&str, FlowImpact>) -> Option<FlowImpact> {
    let mut combined = None;

    for id in flow.referenced_request_ids() {
        if let Some(&impact) = removed.get(id) {
            combined = combine_impact(combined, Some(impact));
            if combined == Some(FlowImpact::Mixed) {
                break;
            }
        }
    }

    combined
}

fn combine_impact(a: Option<FlowImpact>, b: Option<FlowImpact>) -> Option<FlowImpact> {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(_), Some(_)) => Some(FlowImpact::Mixed),
    }
}

fn collect_files<'a>(entries: &'a [Entry], files: &mut Vec<&'a FileEntry>) {
    for entry in entries {
        match entry {
            Entry::File(file) => files.push(file),
            Entry::Directory(folder) => collect_files(&folder.entries, files),
        }
    }
}

fn collect_files_at<'a>(entries: &'a [Entry], path: &Path, files: &mut Vec<&'a FileEntry>) -> bool {
    for entry in entries {
        match entry {
            Entry::File(file) if file.path == path => {
                files.push(file);
                return true;
            }
            Entry::Directory(folder) if folder.path == path => {
                collect_files(&folder.entries, files);
                return true;
            }
            Entry::Directory(folder)
                if path.starts_with(&folder.path)
                    && collect_files_at(&folder.entries, path, files) =>
            {
                return true;
            }
            _ => {}
        }
    }

    false
}
