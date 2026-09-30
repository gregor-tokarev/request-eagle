use crate::variable_input::{active_token, variable_references};

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

#[test]
fn finds_references_that_sending_resolves() {
    let references = |text| variable_references(text).collect::<Vec<_>>();

    assert_eq!(
        references("🦅{{base_url}}/users/{{ id }}?t={{$timestamp}}"),
        vec![4..16, 23..31, 34..48]
    );
    assert_eq!(references("{{!literal}}/{{name}}"), vec![13..21]);
    assert_eq!(references("{{unclosed/{{name}}"), vec![0..19]);
    assert_eq!(
        references("{\n  \"a\": \"{{\",\n  \"b\": \"{{b}}\"\n}"),
        vec![23..28]
    );
    assert_eq!(references("{{}} {name} {{open"), vec![0..4]);
    assert!(references("plain").is_empty());
}
