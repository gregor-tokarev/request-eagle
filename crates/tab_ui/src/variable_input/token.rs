use std::ops::Range;

/// A token around the caret, including any existing suffix and closing braces.
/// Offsets use UTF-8 bytes, matching GPUI's selection API.
pub(super) fn active_token(text: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let before = text.get(..cursor)?;
    let start = before.rfind("{{")?;
    let query = &before[start + 2..];

    if query
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '{' | '}' | '"' | '\''))
    {
        return None;
    }

    let suffix = &text[cursor..];
    let name_end = suffix
        .find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '$' | ':')))
        .unwrap_or(suffix.len());
    let mut end = cursor + name_end;
    if text[end..].starts_with("}}") {
        end += 2;
    } else if text[end..].starts_with('}') {
        end += 1;
    }

    Some((start..end, query))
}
