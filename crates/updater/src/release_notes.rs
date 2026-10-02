use std::sync::OnceLock;

use serde::Deserialize;

const EMBEDDED: &str = include_str!(concat!(env!("OUT_DIR"), "/release-notes.json"));

/// The pull requests merged since the previous release.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReleaseNotes {
    pub version: String,
    /// The release date, as YYYY-MM-DD.
    pub date: String,
    /// In the order they were merged.
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Change {
    pub pull_request: u32,
    pub title: String,
}

/// Every release up to this build, newest first. Only builds made by the
/// release workflow have notes.
pub fn release_notes() -> &'static [ReleaseNotes] {
    static NOTES: OnceLock<Vec<ReleaseNotes>> = OnceLock::new();

    NOTES.get_or_init(|| serde_json::from_str(EMBEDDED).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{EMBEDDED, ReleaseNotes};

    /// CI embeds notes generated from this repository, so this checks that the
    /// generator and the app agree on their shape.
    #[test]
    fn embedded_notes_match_the_app_schema() {
        let notes: Vec<ReleaseNotes> = serde_json::from_str(EMBEDDED).unwrap();

        assert!(notes.iter().all(|release| !release.version.is_empty()));
    }
}
