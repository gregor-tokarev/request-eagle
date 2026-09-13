use std::{
    fs,
    path::{Path, PathBuf},
};

use super::mutations::rebase_entries;
use crate::{CollectionEditError, CollectionRegistry, order};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovePlacement {
    Before,
    After,
    Inside,
}

impl CollectionRegistry {
    pub fn move_entry(
        &mut self,
        source: &Path,
        target: &Path,
        placement: MovePlacement,
    ) -> Result<PathBuf, CollectionEditError> {
        if source == target {
            return Err(CollectionEditError::InvalidMove);
        }
        let source_parent = source.parent().ok_or(CollectionEditError::NotFound)?;
        let source_entries = self
            .entries_mut(source_parent)
            .ok_or(CollectionEditError::NotFound)?;
        let source_index = source_entries
            .iter()
            .position(|entry| entry.path() == source)
            .ok_or(CollectionEditError::NotFound)?;
        let parent = if placement == MovePlacement::Inside {
            target
        } else {
            target.parent().ok_or(CollectionEditError::NotFound)?
        };
        if parent.starts_with(source) {
            return Err(CollectionEditError::InvalidMove);
        }

        let entries = self
            .entries_mut(parent)
            .ok_or(CollectionEditError::NotFound)?;
        let mut paths: Vec<_> = entries
            .iter()
            .filter(|entry| entry.path() != source)
            .map(|entry| entry.path().to_path_buf())
            .collect();
        let index = match placement {
            MovePlacement::Inside => paths.len(),
            MovePlacement::Before | MovePlacement::After => {
                paths
                    .iter()
                    .position(|path| path == target)
                    .ok_or(CollectionEditError::NotFound)?
                    + usize::from(placement == MovePlacement::After)
            }
        };
        let destination = parent.join(source.file_name().ok_or(CollectionEditError::NotFound)?);
        if destination != source {
            match fs::symlink_metadata(&destination) {
                Ok(_) => return Err(CollectionEditError::AlreadyExists),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        fs::symlink_metadata(source)?;
        paths.insert(index, destination.clone());
        order::with_saved_order(parent, &paths, || {
            if destination != source {
                fs::rename(source, &destination)?;
            }
            Ok(())
        })?;

        let mut entry = self
            .entries_mut(source_parent)
            .unwrap()
            .remove(source_index);
        rebase_entries(std::slice::from_mut(&mut entry), source, &destination);
        self.entries_mut(parent).unwrap().insert(index, entry);
        Ok(destination)
    }
}
