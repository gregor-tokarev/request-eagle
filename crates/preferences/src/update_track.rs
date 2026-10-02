use serde::{Deserialize, Serialize};

/// Which releases the app updates to.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateTrack {
    /// Nightly builds that were promoted by hand after use.
    #[default]
    Stable,
    /// A new build of the main branch most days.
    Nightly,
}
