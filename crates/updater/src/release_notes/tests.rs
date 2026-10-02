use super::{EMBEDDED, ReleaseNotes};

/// CI embeds notes generated from this repository, so this checks that the
/// generator and the app agree on their shape.
#[test]
fn embedded_notes_match_the_app_schema() {
    let notes: Vec<ReleaseNotes> = serde_json::from_str(EMBEDDED).unwrap();

    assert!(notes.iter().all(|release| !release.version.is_empty()));
}
