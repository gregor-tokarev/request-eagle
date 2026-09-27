use super::token::active_token;

#[test]
fn finds_token_at_caret_and_replaces_existing_suffix_without_eating_surroundings() {
    assert_eq!(active_token("{{", 2), Some((0..2, "")));
    assert_eq!(
        active_token("🦅/{{base_url}}/tail", 10),
        Some((5..17, "bas"))
    );
    assert_eq!(active_token("{{one}}/{{two", 12), Some((8..13, "tw")));
    assert_eq!(active_token("{{host}/tail", 6), Some((0..7, "host")));
    assert_eq!(active_token("{", 1), None);
    assert_eq!(active_token("{{host}}", 8), None);
    assert_eq!(active_token("{{bad name", 10), None);
    assert_eq!(active_token("🦅", 1), None);
}
