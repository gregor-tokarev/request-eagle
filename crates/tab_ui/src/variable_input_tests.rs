use gpui_kit::{Bounds, point, px, size};

use crate::variable_input::{active_token, chip_bounds, variable_references};

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

#[test]
fn finds_references_in_one_pass_over_large_bodies() {
    // Every unclosed `{{` used to search the rest of the body for `}}`.
    let body = "{{open\n".repeat(200_000) + "{{end}}";
    let start = body.len() - 7;

    assert_eq!(
        variable_references(&body).collect::<Vec<_>>(),
        vec![start..body.len()]
    );
}

#[test]
fn measures_chips_on_one_visible_row() {
    // One 10 px column per byte and 20 px rows: `{{a}}` at 0..5, `{{b}}` at
    // 10..15 ends where the text soft-wraps, `{{c}}` at 20..25 wraps inside,
    // `{{d}}` at 30..35 is folded and `{{e}}` at 40..45 is scrolled away.
    let position = |offset: usize| match offset {
        0..=14 => Some(point(px(offset as f32 * 10.), px(0.))),
        15..=22 => Some(point(px((offset - 15) as f32 * 10.), px(20.))),
        23..=29 => Some(point(px((offset - 23) as f32 * 10.), px(40.))),
        30..=35 => Some(point(px(0.), px(60.))),
        _ => None,
    };
    let range_to_bounds = |range: &std::ops::Range<usize>| {
        let start = position(range.start)?;
        let end = position(range.end)?;
        Some(Bounds::from_corners(start, end + point(px(0.), px(20.))))
    };
    let chips = [0..5, 10..15, 20..25, 30..35, 40..45];

    assert_eq!(
        chip_bounds(&chips, Some(px(20.)), range_to_bounds),
        vec![
            Bounds::new(point(px(0.), px(0.)), size(px(50.), px(20.))),
            Bounds::new(point(px(100.), px(0.)), size(px(50.), px(20.))),
        ]
    );
    assert!(chip_bounds(&chips, None, range_to_bounds).is_empty());
}
