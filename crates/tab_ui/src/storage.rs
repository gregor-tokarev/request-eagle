use collection::SavedLocation;
use gpui_kit::SharedString;

/// Whether a request draft's request is saved, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Not saved yet, with the name given in its tab, if any.
    Unsaved {
        name: Option<SharedString>,
    },
    Saved(SavedLocation),
}

impl Storage {
    pub fn location(&self) -> Option<&SavedLocation> {
        match self {
            Self::Saved(location) => Some(location),
            Self::Unsaved { .. } => None,
        }
    }

    /// The saved name, or the name given in the tab before saving.
    pub fn name(&self) -> Option<SharedString> {
        match self {
            Self::Saved(location) => Some(location.name.clone().into()),
            Self::Unsaved { name } => name.clone(),
        }
    }

    /// Naming a request before it is saved is an unsaved change too.
    pub fn is_dirty(&self) -> bool {
        matches!(self, Self::Unsaved { name: Some(_) })
    }
}
