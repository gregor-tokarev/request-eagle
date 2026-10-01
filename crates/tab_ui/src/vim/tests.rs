#[test]
fn grapheme_navigation_crosses_rope_chunks_in_both_directions() {
    use unicode_segmentation::UnicodeSegmentation as _;
    let source = "e\u{301}👩\u{200d}🚀🇺🇸\r\n".repeat(500);
    let rope = gpui_kit::component::input::Rope::from(source.clone());
    assert!(rope.chunks().count() > 1);
    let mut expected: Vec<_> = source
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect();
    expected.push(source.len());

    for pair in expected.windows(2) {
        assert_eq!(super::motions::next(&rope, pair[0]), pair[1]);
        assert_eq!(super::motions::previous(&rope, pair[1]), pair[0]);
    }
}
