use gpui_kit::{
    component::{ActiveTheme as _, input::Rope},
    *,
};

use super::motions::{line, next, normal_cursor};

pub(super) enum Column {
    Position {
        prefix: std::ops::Range<usize>,
        width: Option<Pixels>,
    },
    End,
}

impl Column {
    pub(super) fn at(text: &Rope, cursor: usize) -> Self {
        Self::Position {
            prefix: line(text, cursor).start..cursor,
            width: None,
        }
    }

    pub(super) fn offset(
        &mut self,
        text: &Rope,
        cursor: usize,
        window: &Window,
        cx: &App,
    ) -> usize {
        let range = line(text, cursor);
        let offset = match self {
            Self::End => range.end,
            Self::Position { prefix, width } => {
                // Vim clears the desired column whenever the document changes.
                // Returning to the source line needs no measurement at all.
                if range.start == prefix.start {
                    return normal_cursor(text, prefix.end);
                }

                if prefix.start == prefix.end || range.is_empty() {
                    return range.start;
                }

                // Measure both prefixes lazily. Once the destination ends before
                // the desired column, the rest of a long source line is irrelevant.
                // Remember the source position across shorter lines without
                // copying or shaping its complete prefix upfront.
                let mut length = 64;

                loop {
                    let source_end = prefix_end(text, prefix, length);
                    let x = width.unwrap_or_else(|| {
                        shape(text.slice(prefix.start..source_end).to_string(), window, cx).width
                    });

                    if source_end == prefix.end {
                        *width = Some(x);
                    }

                    let end = prefix_end(text, &range, length);
                    let shaped = shape(text.slice(range.start..end).to_string(), window, cx);

                    if width.is_some() && shaped.width >= x {
                        break range.start + shaped.closest_index_for_x(x);
                    }

                    if end == range.end && x >= shaped.width {
                        break range.end;
                    }

                    length = length.saturating_mul(2);
                }
            }
        };
        normal_cursor(text, offset)
    }
}

fn prefix_end(text: &Rope, range: &std::ops::Range<usize>, length: usize) -> usize {
    let mut end = range.start.saturating_add(length).min(range.end);

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    next(text, end).min(range.end)
}

fn shape(text: String, window: &Window, cx: &App) -> ShapedLine {
    // Match the body/script editors' font and text_sm size. Measuring the actual
    // layout also respects platform tab expansion and wide/combined glyphs.
    let run = TextRun {
        len: text.len(),
        font: font(cx.theme().mono_font_family.clone()),
        ..Default::default()
    };
    window.text_system().shape_line(
        text.into(),
        rems(0.875).to_pixels(window.rem_size()),
        &[run],
        None,
    )
}
