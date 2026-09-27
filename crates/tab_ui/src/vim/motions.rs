use gpui_kit::component::input::{Rope, RopeExt};
use std::ops::Range;

pub(super) struct Motion {
    pub offset: usize,
    pub inclusive: bool,
    pub linewise: bool,
}

pub(super) fn next(text: &Rope, offset: usize) -> usize {
    offset + text.char_at(offset).map_or(0, char::len_utf8)
}

pub(super) fn previous(text: &Rope, offset: usize) -> usize {
    offset
        - text
            .chars_at(offset)
            .reversed()
            .next()
            .map_or(0, char::len_utf8)
}

pub(super) fn line(text: &Rope, offset: usize) -> Range<usize> {
    let row = text.offset_to_point(offset).row;
    let start = text.line_start_offset(row);
    let mut end = text.line_end_offset(row);

    if end > start && text.char_at(previous(text, end)) == Some('\r') {
        end -= 1;
    }

    start..end
}

pub(super) fn normal_cursor(text: &Rope, offset: usize) -> usize {
    let line = line(text, offset.min(text.len()));
    offset.min(if line.is_empty() {
        line.start
    } else {
        previous(text, line.end)
    })
}

pub(super) fn first_nonblank(text: &Rope, offset: usize) -> usize {
    let line = line(text, offset);
    let mut position = line.start;

    while position < line.end && text.char_at(position).is_some_and(char::is_whitespace) {
        position = next(text, position);
    }

    position
}

pub(super) fn lines(text: &Rope, from: usize, to: usize) -> Range<usize> {
    let start = line(text, from.min(to)).start;
    let row = text.offset_to_point(from.max(to)).row;
    let end = if row + 1 < text.lines_len() {
        text.line_start_offset(row + 1)
    } else {
        text.len()
    };

    start..end
}

fn word_class(character: Option<char>, big: bool) -> u8 {
    match character {
        None => 0,
        Some(character) if character.is_whitespace() => 0,
        Some(character) if big || character.is_alphanumeric() || character == '_' => 1,
        _ => 2,
    }
}

pub(super) fn motion(text: &Rope, cursor: usize, key: &str, count: usize) -> Option<Motion> {
    let count = count.max(1);
    let mut offset = cursor;
    let mut inclusive = false;
    let mut linewise = false;

    match key {
        "h" | "left" => {
            for _ in 0..count {
                offset = previous(text, offset).max(line(text, cursor).start);
            }
        }
        "l" | "right" | "space" => {
            for _ in 0..count {
                offset = next(text, offset).min(line(text, cursor).end);
            }
        }
        "j" | "down" | "k" | "up" => {
            let current = text.offset_to_point(cursor);
            let column = text
                .slice(text.line_start_offset(current.row)..cursor)
                .chars()
                .count();
            let row = if matches!(key, "j" | "down") {
                current.row.saturating_add(count).min(text.lines_len() - 1)
            } else {
                current.row.saturating_sub(count)
            };
            let target = line(text, text.line_start_offset(row));
            offset = target.start;

            for _ in 0..column {
                offset = next(text, offset).min(target.end);
            }

            linewise = true;
        }
        "0" | "home" => offset = line(text, cursor).start,
        "^" => offset = first_nonblank(text, cursor),
        "$" | "end" => {
            let row = (text.offset_to_point(cursor).row + count - 1).min(text.lines_len() - 1);
            let target = line(text, text.line_start_offset(row));
            offset = normal_cursor(text, target.end);
            inclusive = true;
        }
        "gg" | "G" => {
            let row = (count - 1).min(text.lines_len() - 1);
            offset = first_nonblank(text, text.line_start_offset(row));
            linewise = true;
        }
        "w" | "W" => {
            for _ in 0..count {
                let class = word_class(text.char_at(offset), key == "W");

                while offset < text.len() && word_class(text.char_at(offset), key == "W") == class {
                    offset = next(text, offset);
                }

                while offset < text.len() && word_class(text.char_at(offset), true) == 0 {
                    offset = next(text, offset);
                }
            }
        }
        "b" | "B" => {
            for _ in 0..count {
                offset = previous(text, offset);

                while offset > 0 && word_class(text.char_at(offset), true) == 0 {
                    offset = previous(text, offset);
                }

                let class = word_class(text.char_at(offset), key == "B");

                while offset > 0
                    && word_class(text.char_at(previous(text, offset)), key == "B") == class
                {
                    offset = previous(text, offset);
                }
            }
        }
        "e" | "E" | "cw" | "cW" => {
            let big = matches!(key, "E" | "cW");

            for index in 0..count {
                if index > 0 || matches!(key, "e" | "E") {
                    offset = next(text, offset);
                }

                while offset < text.len() && word_class(text.char_at(offset), true) == 0 {
                    offset = next(text, offset);
                }

                let class = word_class(text.char_at(offset), big);

                while next(text, offset) < text.len()
                    && word_class(text.char_at(next(text, offset)), big) == class
                {
                    offset = next(text, offset);
                }
            }

            inclusive = true;
        }
        _ => return None,
    }

    Some(Motion {
        offset,
        inclusive,
        linewise,
    })
}
